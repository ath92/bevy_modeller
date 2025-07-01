//! SDF compute module for GPU-accelerated SDF evaluation
//!
//! This module provides functionality to evaluate SDF values for arbitrary points
//! using compute shaders. It's designed to work with the existing SDF rendering pipeline
//! and shares the same scene data (entity transforms and settings).

use bevy::{
    prelude::*,
    render::{
        extract_component::ComponentUniforms,
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderQueue},
        Render, RenderApp, RenderSet,
    },
};
use crossbeam_channel::{Receiver, Sender};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

const SHADER_ASSET_PATH: &str = "shaders/sdf_compute.wgsl";

/// Result of SDF evaluation matching the WGSL SceneSdfResult struct
#[repr(C)]
#[derive(
    Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, bevy::render::render_resource::ShaderType,
)]
pub struct SdfResult {
    pub distance: f32,
    // pub position: Vec3,
}

/// Request for SDF evaluation
#[derive(Debug, Clone)]
pub struct SdfEvaluationRequest {
    pub points: Vec<Vec2>,
    pub id: u64,
}

/// Response from SDF evaluation
#[derive(Debug, Clone)]
pub struct SdfEvaluationResponse {
    pub results: Vec<SdfResult>,
    pub id: u64,
}

/// Resource for sending SDF evaluation requests
#[derive(Resource)]
pub struct SdfEvaluationSender(pub Sender<SdfEvaluationRequest>);

/// Resource for receiving SDF evaluation responses
#[derive(Resource)]
pub struct SdfEvaluationReceiver(pub Receiver<SdfEvaluationResponse>);

/// Resource for sending responses from render world to main world
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<SdfEvaluationResponse>);

/// Resource for receiving requests in render world
#[derive(Resource, Deref)]
struct RenderWorldReceiver(Receiver<SdfEvaluationRequest>);

/// GPU-aligned Vec3 for proper buffer alignment (16 bytes instead of 12)
#[repr(C)]
#[derive(
    Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, bevy::render::render_resource::ShaderType,
)]
pub struct GpuVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub _padding: f32, // Padding to align to 16 bytes
}

/// Global counter for request IDs
static REQUEST_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Plugin for SDF compute functionality
pub struct SdfComputePlugin;

impl Plugin for SdfComputePlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let (request_sender, request_receiver) = crossbeam_channel::unbounded();
        let (response_sender, response_receiver) = crossbeam_channel::unbounded();

        app.insert_resource(SdfEvaluationSender(request_sender))
            .insert_resource(SdfEvaluationReceiver(response_receiver));

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .insert_resource(RenderWorldReceiver(request_receiver))
            .insert_resource(RenderWorldSender(response_sender))
            .init_resource::<SdfComputePipeline>()
            .init_resource::<SdfComputeBuffers>()
            .init_resource::<PendingSdfRequests>()
            .add_systems(
                Render,
                (
                    prepare_sdf_bind_groups
                        .in_set(RenderSet::PrepareBindGroups)
                        .run_if(
                            resource_exists::<
                                ComponentUniforms<crate::post_process::PostProcessSettings>,
                            >,
                        ),
                    process_sdf_requests.before(RenderSet::Render),
                    perform_gpu_readback.after(RenderSet::Render),
                ),
            );

        // Add the compute node to the render graph
        render_app
            .world_mut()
            .resource_mut::<RenderGraph>()
            .add_node(SdfComputeNodeLabel, SdfComputeNode::default());
    }
}

#[derive(Resource)]
struct SdfComputeBuffers {
    query_points_buffer: Buffer,
    results_buffer: Buffer,
    readback_buffer: Buffer,
    current_capacity: usize,
}

impl FromWorld for SdfComputeBuffers {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let initial_capacity = 1024; // Start with capacity for 1024 points

        let query_points_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("sdf_query_points_buffer"),
            size: (initial_capacity * std::mem::size_of::<Vec2>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let results_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("sdf_results_buffer"),
            size: (initial_capacity * std::mem::size_of::<SdfResult>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let readback_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("sdf_readback_buffer"),
            size: (initial_capacity * std::mem::size_of::<SdfResult>()) as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            query_points_buffer,
            results_buffer,
            readback_buffer,
            current_capacity: initial_capacity,
        }
    }
}

#[derive(Resource)]
struct SdfComputeBindGroups {
    compute_bind_group: BindGroup,
    sdf_bind_group: BindGroup,
}

fn prepare_sdf_bind_groups(
    mut commands: Commands,
    pipeline: Res<SdfComputePipeline>,
    render_device: Res<RenderDevice>,
    buffers: Res<SdfComputeBuffers>,
    entity_buffer: Res<crate::post_process::EntityTransformBuffer>,
    settings_uniforms: Res<ComponentUniforms<crate::post_process::PostProcessSettings>>,
) {
    // Bind group 0: compute-specific resources (query points and results)
    let compute_bind_group = render_device.create_bind_group(
        Some("sdf_compute_bind_group"),
        &pipeline.compute_layout,
        &BindGroupEntries::sequential((
            buffers.query_points_buffer.as_entire_binding(),
            buffers.results_buffer.as_entire_binding(),
        )),
    );

    // Bind group 1: shared SDF scene data (from post_process module)
    // Use the actual settings uniform from the post_process module
    if let Some(settings_binding) = settings_uniforms.uniforms().binding() {
        let sdf_bind_group = render_device.create_bind_group(
            Some("sdf_scene_bind_group"),
            &pipeline.sdf_layout,
            &BindGroupEntries::sequential((
                settings_binding,
                entity_buffer.buffer.as_ref().unwrap().as_entire_binding(),
            )),
        );

        commands.insert_resource(SdfComputeBindGroups {
            compute_bind_group,
            sdf_bind_group,
        });
    }
}

#[derive(Resource)]
struct SdfComputePipeline {
    compute_layout: BindGroupLayout,
    sdf_layout: BindGroupLayout,
    pipeline: CachedComputePipelineId,
}

impl FromWorld for SdfComputePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        // Bind group 0: compute-specific resources
        let compute_layout = render_device.create_bind_group_layout(
            Some("sdf_compute_layout"),
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    // Query points buffer
                    storage_buffer_read_only::<Vec2>(false),
                    // Results buffer
                    storage_buffer::<SdfResult>(false),
                ),
            ),
        );

        // Bind group 1: shared SDF scene data (matches sdf_common.wgsl)
        let sdf_layout = render_device.create_bind_group_layout(
            Some("sdf_scene_layout"),
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    // PostProcessSettings uniform
                    uniform_buffer::<crate::post_process::PostProcessSettings>(true),
                    // Entity transforms storage buffer
                    storage_buffer_read_only::<Mat4>(false),
                ),
            ),
        );

        let shader = world.load_asset(SHADER_ASSET_PATH);
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("sdf_compute_pipeline".into()),
            layout: vec![compute_layout.clone(), sdf_layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: shader.clone(),
            shader_defs: Vec::new(),
            entry_point: "main".into(),
            zero_initialize_workgroup_memory: true,
        });

        SdfComputePipeline {
            compute_layout,
            sdf_layout,
            pipeline,
        }
    }
}

/// Pending SDF requests waiting for GPU readback
#[derive(Resource, Default)]
struct PendingSdfRequests {
    requests: Vec<(SdfEvaluationRequest, usize)>, // (request, points_count)
    pending_mapping: Option<(SdfEvaluationRequest, usize, crossbeam_channel::Receiver<()>)>, // (request, points_count, receiver)
}

fn process_sdf_requests(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut buffers: ResMut<SdfComputeBuffers>,
    mut pending_requests: ResMut<PendingSdfRequests>,
    receiver: Res<RenderWorldReceiver>,
) {
    // Process new incoming requests
    while let Some(request) = receiver.try_recv() {
        let points_count = request.points.len();
        if points_count == 0 {
            continue;
        }

        // Resize buffers if needed
        if points_count > buffers.current_capacity {
            let new_capacity = (points_count * 2).max(1024);

            buffers.query_points_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some("sdf_query_points_buffer"),
                size: (new_capacity * std::mem::size_of::<Vec2>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            buffers.results_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some("sdf_results_buffer"),
                size: (new_capacity * std::mem::size_of::<SdfResult>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

            buffers.readback_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some("sdf_readback_buffer"),
                size: (new_capacity * std::mem::size_of::<SdfResult>()) as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            buffers.current_capacity = new_capacity;
        }

        // Upload query points to GPU
        let points_data = bytemuck::cast_slice(&request.points);
        render_queue.write_buffer(&buffers.query_points_buffer, 0, points_data);

        // Add to pending requests for GPU readback after compute dispatch
        pending_requests.requests.push((request, points_count));
    }
}

fn perform_gpu_readback(
    render_device: Res<RenderDevice>,
    buffers: Res<SdfComputeBuffers>,
    mut pending_requests: ResMut<PendingSdfRequests>,
    sender: Res<RenderWorldSender>,
) {
    // Use non-blocking poll to advance GPU operations
    render_device.poll(Maintain::Poll);

    // First, check if we have a pending mapping
    if let Some((request, points_count, rx)) = pending_requests.pending_mapping.take() {
        // Check if mapping is complete (non-blocking)
        match rx.try_recv() {
            Some(_) => {
                // Read the data
                let buffer_slice = buffers.readback_buffer.slice(..);
                let mapped_range = buffer_slice.get_mapped_range();

                const RESULT_SIZE: usize = std::mem::size_of::<SdfResult>();

                let results_data = mapped_range
                    .chunks_exact(RESULT_SIZE)
                    .take(points_count)
                    .map(|chunk| {
                        let bytes: [u8; RESULT_SIZE] = chunk.try_into().unwrap();

                        let result = bytemuck::from_bytes::<SdfResult>(&bytes).clone();
                        info!("{:?} res", result);
                        return result;
                    })
                    .collect::<Vec<_>>();

                info!("result {:?}", results_data);

                drop(mapped_range);
                buffers.readback_buffer.unmap();

                let response = SdfEvaluationResponse {
                    results: results_data,
                    id: request.id,
                };

                info!("res {:?}", response);

                let _ = sender.send(response);
            }
            None => {
                // Mapping not ready yet, keep it for next frame
                info!("mapping not ready, keeping for next frame");
                pending_requests.pending_mapping = Some((request, points_count, rx));
                return;
            }
        }
    }

    // If no pending mapping, start a new one if we have requests
    if pending_requests.requests.is_empty() {
        return;
    }

    info!("starting new readback");

    // Process one request at a time to avoid complexity
    let (request, points_count) = pending_requests.requests.remove(0);

    // Map the readback buffer to read results
    let buffer_slice = buffers.readback_buffer.slice(..);

    let (tx, rx) = crossbeam_channel::unbounded::<()>();

    buffer_slice.map_async(MapMode::Read, move |result| match result {
        Ok(_) => {
            let _ = tx.send(());
        }
        Err(err) => {
            eprintln!("Failed to map buffer: {:?}", err);
            let _ = tx.send(());
        }
    });

    // Store the pending mapping for next frame
    pending_requests.pending_mapping = Some((request, points_count, rx));
}

/// Label to identify the SDF compute node in the render graph
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct SdfComputeNodeLabel;

/// The node that executes the SDF compute shader
#[derive(Default)]
struct SdfComputeNode;

impl render_graph::Node for SdfComputeNode {
    fn run(
        &self,
        _graph: &mut render_graph::RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<SdfComputePipeline>();

        if let Some(bind_groups) = world.get_resource::<SdfComputeBindGroups>() {
            if let Some(compute_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) {
                let mut pass =
                    render_context
                        .command_encoder()
                        .begin_compute_pass(&ComputePassDescriptor {
                            label: Some("sdf_compute_pass"),
                            ..default()
                        });

                let settings_index = 0;

                pass.set_bind_group(0, &bind_groups.compute_bind_group, &[]);
                pass.set_bind_group(1, &bind_groups.sdf_bind_group, &[settings_index]);
                pass.set_pipeline(compute_pipeline);

                // Dispatch workgroups based on pending requests
                let pending_requests = world.resource::<PendingSdfRequests>();
                if !pending_requests.requests.is_empty() {
                    let max_points = pending_requests
                        .requests
                        .iter()
                        .map(|(_, count)| *count)
                        .max()
                        .unwrap_or(0);
                    let workgroups = (max_points as u32 + 63) / 64; // 64 threads per workgroup

                    pass.dispatch_workgroups(workgroups, 1, 1);
                }
            }
        }

        // Copy results buffer to readback buffer after compute
        let buffers = world.resource::<SdfComputeBuffers>();
        let pending_requests = world.resource::<PendingSdfRequests>();

        if !pending_requests.requests.is_empty() {
            render_context.command_encoder().copy_buffer_to_buffer(
                &buffers.results_buffer,
                0,
                &buffers.readback_buffer,
                0,
                (buffers.current_capacity * std::mem::size_of::<SdfResult>()) as u64,
            );
        }

        Ok(())
    }
}

/// Future that resolves when SDF evaluation is complete
pub struct SdfEvaluationFuture {
    receiver: Arc<Mutex<Option<Receiver<SdfEvaluationResponse>>>>,
    id: u64,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Future for SdfEvaluationFuture {
    type Output = Vec<SdfResult>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut waker = self.waker.lock().unwrap();
        *waker = Some(cx.waker().clone());
        drop(waker);

        let mut receiver_opt = self.receiver.lock().unwrap();
        if let Some(receiver) = receiver_opt.as_ref() {
            match receiver.try_recv() {
                Some(response) if response.id == self.id => {
                    *receiver_opt = None;
                    Poll::Ready(response.results)
                }
                Some(_) => Poll::Pending, // Wrong ID, keep waiting
                None => Poll::Pending,    // No data yet
            }
        } else {
            Poll::Pending
        }
    }
}

/// Public API function to evaluate SDF at given points (async)
pub fn evaluate_sdf_async(
    points: Vec<Vec2>,
    sender: &SdfEvaluationSender,
    receiver: &SdfEvaluationReceiver,
) -> SdfEvaluationFuture {
    let id = REQUEST_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let request = SdfEvaluationRequest { points, id };

    let _ = sender.0.send(request);

    SdfEvaluationFuture {
        receiver: Arc::new(Mutex::new(Some(receiver.0.clone()))),
        id,
        waker: Arc::new(Mutex::new(None)),
    }
}
