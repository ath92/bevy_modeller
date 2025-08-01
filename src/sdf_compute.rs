//! SDF compute module for GPU-accelerated SDF evaluation
//!
//! This module provides functionality to evaluate SDF values for arbitrary points
//! using compute shaders. It's designed to work with the existing SDF rendering pipeline
//! and shares the same scene data (entity transforms and settings).

use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    prelude::*,
    render::{
        extract_component::ComponentUniforms,
        render_graph::{self, RenderGraphApp, RenderLabel},
        render_resource::{
            binding_types::*, BindGroupLayoutEntry, BindingType, BufferBindingType, ShaderStages, *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        Render, RenderApp, RenderSet,
    },
};
use crossbeam_channel;
use futures::channel::oneshot;

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
#[derive(Debug)]
pub struct SdfEvaluationRequest {
    pub points: Vec<Vec2>,
    pub response_tx: oneshot::Sender<Vec<SdfResult>>,
}

/// Resource for sending SDF evaluation requests to render world
#[derive(Resource, Clone)]
pub struct SdfEvaluationSender(pub crossbeam_channel::Sender<SdfEvaluationRequest>);

/// Resource for receiving requests in render world
#[derive(Resource, Deref)]
struct RenderWorldReceiver(crossbeam_channel::Receiver<SdfEvaluationRequest>);

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

/// Plugin for SDF compute functionality
pub struct SdfComputePlugin;

impl Plugin for SdfComputePlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let (request_sender, request_receiver) = crossbeam_channel::unbounded();

        app.insert_resource(SdfEvaluationSender(request_sender));

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .insert_resource(RenderWorldReceiver(request_receiver))
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
                                ComponentUniforms<crate::sdf_render::SDFRenderSettings>,
                            >,
                        ),
                    process_sdf_requests.before(RenderSet::Render),
                    initiate_gpu_readback.after(RenderSet::Render),
                    perform_delayed_readback.in_set(RenderSet::PostCleanup),
                ),
            );

        // Add the compute node to the render graph
        render_app
            .add_render_graph_node::<SdfComputeNode>(Core3d, SdfComputeNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::EndMainPassPostProcessing, SdfComputeNodeLabel),
            );
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

        info!("create buffers");
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
    entity_buffer: Res<crate::sdf_render::EntityBuffer>,
    bvh_buffer: Res<crate::sdf_render::BVHBuffer>,
    settings_uniforms: Res<ComponentUniforms<crate::sdf_render::SDFRenderSettings>>,
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
        if let Some(bvh_buffer_binding) = bvh_buffer.buffer.as_ref().map(|b| b.as_entire_binding())
        {
            let sdf_bind_group = render_device.create_bind_group(
                Some("sdf_scene_bind_group"),
                &pipeline.sdf_layout,
                &BindGroupEntries::sequential((
                    settings_binding,
                    entity_buffer.buffer.as_ref().unwrap().as_entire_binding(),
                    bvh_buffer_binding,
                )),
            );

            commands.insert_resource(SdfComputeBindGroups {
                compute_bind_group,
                sdf_bind_group,
            });
        }
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
                    uniform_buffer::<crate::sdf_render::SDFRenderSettings>(true),
                    // Entity transforms storage buffer
                    storage_buffer_read_only::<Mat4>(false),
                    // BVH nodes storage buffer
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
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
    requests: Vec<SdfEvaluationRequest>,
    completed_requests: Vec<SdfEvaluationRequest>,
    pending_mapping: Option<(SdfEvaluationRequest, crossbeam_channel::Receiver<()>)>, // (request, receiver)
    ready_for_mapping: Vec<SdfEvaluationRequest>,
}

fn process_sdf_requests(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut buffers: ResMut<SdfComputeBuffers>,
    mut pending_requests: ResMut<PendingSdfRequests>,
    receiver: ResMut<RenderWorldReceiver>,
) {
    // Process new incoming requests
    while let Some(request) = receiver.try_recv() {
        // info!(
        //     "Received SDF request with ID: {} for {} points",
        //     request.id,
        //     request.points.len()
        // );
        let points_count = request.points.len();
        if points_count == 0 {
            info!("Skipping empty SDF request");
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
        // info!("Adding SDF request ID: {} to pending queue", request.id);
        pending_requests.requests.push(request);
    }
}

fn initiate_gpu_readback(mut pending_requests: ResMut<PendingSdfRequests>) {
    // Move completed requests from the main queue if needed
    if pending_requests.completed_requests.is_empty() && !pending_requests.requests.is_empty() {
        let requests_to_move: Vec<_> = pending_requests.requests.drain(..).collect();
        pending_requests.completed_requests.extend(requests_to_move);
    }

    // Move completed requests to ready_for_mapping queue for delayed processing
    if !pending_requests.completed_requests.is_empty() {
        let requests_to_map: Vec<_> = pending_requests.completed_requests.drain(..).collect();
        pending_requests.ready_for_mapping.extend(requests_to_map);
    }
}

fn perform_delayed_readback(
    _render_device: Res<RenderDevice>,
    buffers: Res<SdfComputeBuffers>,
    mut pending_requests: ResMut<PendingSdfRequests>,
) {
    // Check if we have a pending mapping
    if let Some((_request, rx)) = &pending_requests.pending_mapping {
        // Check if mapping is complete (non-blocking)
        match rx.try_recv() {
            Some(_) => {
                // Take the request to process it
                let (request, _) = pending_requests.pending_mapping.take().unwrap();

                // Read the data - wrap in a closure to ensure cleanup on error
                let read_result = (|| -> Result<Vec<SdfResult>, &'static str> {
                    let buffer_slice = buffers.readback_buffer.slice(..);
                    let mapped_range = buffer_slice.get_mapped_range();

                    const RESULT_SIZE: usize = std::mem::size_of::<SdfResult>();
                    let points_count = request.points.len();

                    let mut results_data = Vec::new();
                    for chunk in mapped_range.chunks_exact(RESULT_SIZE).take(points_count) {
                        let bytes: [u8; RESULT_SIZE] = chunk
                            .try_into()
                            .map_err(|_| "Failed to convert chunk to byte array")?;

                        results_data.push(bytemuck::from_bytes::<SdfResult>(&bytes).clone());
                    }

                    Ok(results_data)
                })();

                // Always unmap the buffer regardless of success/failure
                buffers.readback_buffer.unmap();

                // Send results through oneshot channel
                match read_result {
                    Ok(results_data) => {
                        let _ = request.response_tx.send(results_data);
                    }
                    Err(err) => {
                        eprintln!("Failed to read buffer data: {:?}", err);
                        // Send empty results on error
                        let _ = request.response_tx.send(vec![]);
                    }
                }
            }
            None => {
                // Mapping not ready yet, keep waiting
            }
        }
    }

    // Start a new mapping if we have no pending mapping and requests are ready
    if pending_requests.pending_mapping.is_none() && !pending_requests.ready_for_mapping.is_empty()
    {
        // Process one request at a time to minimize GPU impact
        let request = pending_requests.ready_for_mapping.remove(0);

        // Map the readback buffer to read results
        let buffer_slice = buffers.readback_buffer.slice(..);
        let (tx, rx) = crossbeam_channel::unbounded::<()>();
        info!("MAP ASYNC!");
        buffer_slice.map_async(MapMode::Read, move |result| match result {
            Ok(_) => {
                let _ = tx.send(());
            }
            Err(err) => {
                eprintln!("Failed to map buffer: {:?}", err);
                // Send signal even on error so we can cleanup
                let _ = tx.send(());
            }
        });

        // Store the pending mapping for next frame
        pending_requests.pending_mapping = Some((request, rx));
    }
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
                if !pending_requests.requests.is_empty()
                    && pending_requests.pending_mapping.is_none()
                {
                    let max_points = pending_requests
                        .requests
                        .iter()
                        .map(|req| req.points.len())
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

        if !pending_requests.requests.is_empty() && pending_requests.pending_mapping.is_none() {
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

/// Public API function to evaluate SDF at given points (async)
pub async fn evaluate_sdf_async(
    points: Vec<Vec2>,
    sender: &SdfEvaluationSender,
) -> Result<Vec<SdfResult>, oneshot::Canceled> {
    let (response_tx, response_rx) = oneshot::channel();
    let request = SdfEvaluationRequest {
        points,
        response_tx,
    };

    let _ = sender.0.send(request);

    // info!("Awaiting SDF evaluation response for ID: {}", id);
    response_rx.await
}
