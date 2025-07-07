use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        fullscreen_vertex_shader::fullscreen_shader_vertex_state,
        prepass::ViewPrepassTextures,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            Buffer, BufferDescriptor, BufferUsages, *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::ViewTarget,
        Render, RenderApp, RenderSet,
    },
};

use bvh::bounding_hierarchy::{BHShape, BoundingHierarchy};
use bvh::{
    aabb::{Aabb, Bounded},
    bvh::Bvh,
};
use bytemuck::Pod;
use nalgebra::{Point3, Vector3};

/// This example uses a shader source file from the assets subdirectory
const SHADER_ASSET_PATH: &str = "shaders/sdf_render.wgsl";

// Resource to hold transform data in the render world
#[derive(Resource)]
pub struct EntityBuffer {
    pub buffer: Option<Buffer>,
    pub data: Vec<Vec4>,
    pub capacity: usize,
}

// Buffer for BVH data
#[derive(Resource)]
pub struct BVHBuffer {
    pub buffer: Option<Buffer>,
    pub data: Vec<BVHNode>,
    pub capacity: usize,
}

impl Default for EntityBuffer {
    fn default() -> Self {
        Self {
            buffer: None,
            data: Vec::new(),
            capacity: 0,
        }
    }
}

impl Default for BVHBuffer {
    fn default() -> Self {
        Self {
            buffer: None,
            data: Vec::new(),
            capacity: 0,
        }
    }
}

// Component to mark entities whose transforms should be sent to the shader
#[derive(Component, Clone, Debug, PartialEq)]
pub struct SDFRenderEntity {
    pub node_index: usize,
    pub position: Vec3,
    pub scale: f32,
}

impl Bounded<f32, 3> for SDFRenderEntity {
    fn aabb(&self) -> Aabb<f32, 3> {
        let half_size = self.scale + 0.5; // add .5 for smoothing factor - parameterize this?
        let half_size_v3 = Vector3::new(half_size, half_size, half_size);
        let pos = Point3::new(self.position.x, self.position.y, self.position.z);
        let min = pos - half_size_v3;
        let max = pos + half_size_v3;
        Aabb::with_bounds(min, max)
    }
}

impl BHShape<f32, 3> for SDFRenderEntity {
    fn set_bh_node_index(&mut self, index: usize) {
        self.node_index = index;
    }

    fn bh_node_index(&self) -> usize {
        self.node_index
    }
}

// Resource to transfer data from main world to render world
#[derive(Resource, Clone)]
struct EntityData(Vec<Vec4>);

#[repr(C)]
#[derive(Clone, Pod, bytemuck::Zeroable, std::marker::Copy, Debug)]
pub struct BVHNode {
    min: Vec4,
    max: Vec4,
    entry_index: u32,
    exit_index: u32,
    shape_index: u32,
    __padding: u32,
}

// Resource for flattened BVH
#[derive(Resource, Clone)]
struct FlattenedBVH(Vec<BVHNode>);

impl FromWorld for FlattenedBVH {
    fn from_world(_: &mut World) -> Self {
        let v: Vec<BVHNode> = Vec::new();
        Self(v)
    }
}

impl ExtractResource for FlattenedBVH {
    type Source = FlattenedBVH;

    fn extract_resource(source: &Self::Source) -> Self {
        // Create a new FlattenedBVH with the same data
        // Since FlatNode doesn't implement Clone, we need to rebuild it
        source.clone()
    }
}

impl ExtractResource for EntityData {
    type Source = EntityData;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// It is generally encouraged to set up post processing effects as a plugin
pub struct SDFRenderPlugin;

impl Plugin for SDFRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            // The settings will be a component that lives in the main world but will
            // be extracted to the render world every frame.
            // This makes it possible to control the effect from the main world.
            // This plugin will take care of extracting it automatically.
            // It's important to derive [`ExtractComponent`] on [`SDFRenderSettings`]
            // for this plugin to work correctly.
            ExtractComponentPlugin::<SDFRenderSettings>::default(),
            // The settings will also be the data used in the shader.
            // This plugin will prepare the component for the GPU by creating a uniform buffer
            // and writing the data to that buffer every frame.
            UniformComponentPlugin::<SDFRenderSettings>::default(),
            // Extract the EntityTransformData from main world to render world
            ExtractResourcePlugin::<EntityData>::default(),
            // Extract the PostProcessEnabled flag from main world to render world
            ExtractResourcePlugin::<SDFRenderEnabled>::default(),
            // Extract the FlattenedBVH from main world to render world
            ExtractResourcePlugin::<FlattenedBVH>::default(),
        ))
        // Initialize the PostProcessEnabled resource
        .init_resource::<SDFRenderEnabled>()
        // Initialize the FlattenedBVH resource
        .init_resource::<FlattenedBVH>()
        // Add the system to collect transform data
        .add_systems(
            Update,
            (
                sync_entity_positions,
                collect_entity_data,
                update_camera_settings,
                update_entity_count_in_settings,
                update_bvh_node_count_in_settings,
                update_time_in_settings,
                build_entity_bvh.after(collect_entity_data),
            ),
        );

        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<EntityBuffer>()
            // BVH
            .init_resource::<FlattenedBVH>()
            .init_resource::<BVHBuffer>()
            .add_systems(
                Render,
                (
                    manage_coarse_pass_texture.in_set(RenderSet::PrepareResources),
                    update_transform_buffer.in_set(RenderSet::PrepareResources),
                    update_render_world_entity_count
                        .in_set(RenderSet::PrepareResources)
                        .after(update_transform_buffer),
                    update_render_world_bvh_count
                        .in_set(RenderSet::PrepareResources)
                        .after(update_bvh_buffer),
                ),
            )
            .add_systems(
                Render,
                update_bvh_buffer.in_set(RenderSet::PrepareResources),
            )
            .add_render_graph_node::<ViewNodeRunner<SDFCoarsePrepassNode>>(
                Core3d,
                SDFCoarsePrepassLabel,
            )
            .add_render_graph_node::<ViewNodeRunner<SDFRenderNode>>(
                // Specify the label of the graph, in this case we want the graph for 3d
                Core3d,
                // It also needs the label of the node
                SDFRenderLabel,
            )
            .add_render_graph_edges(
                Core3d,
                // Specify the node ordering: Tonemapping -> Coarse Prepass -> Main SDF -> End
                (
                    Node3d::Tonemapping,
                    SDFCoarsePrepassLabel,
                    SDFRenderLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            // Initialize the pipelines
            .init_resource::<SDFRenderPipeline>()
            .init_resource::<FlattenedBVH>()
            .init_resource::<SDFCoarsePrepassPipeline>();
    }
}

// System that runs in the main world to collect transform data
fn collect_entity_data(
    changed_entities: Query<&SDFRenderEntity, Changed<SDFRenderEntity>>,
    all_entities: Query<&SDFRenderEntity>,
    mut commands: Commands,
    entity_data: Option<Res<EntityData>>,
) {
    // Check if we need to collect data
    let needs_update = if entity_data.is_none() {
        // First time - collect all entities
        true
    } else {
        // Only update if entities have changed
        !changed_entities.is_empty()
    };

    if !needs_update {
        return;
    }

    info!(
        "Collecting entity data - {} entities",
        all_entities.iter().count()
    );

    let mut entities: Vec<&SDFRenderEntity> = all_entities.iter().collect();
    entities.sort_by_key(|e| e.node_index);

    let transforms: Vec<Vec4> = entities
        .iter()
        .map(|entity| {
            let translation = entity.position;
            let scale = entity.scale;
            Vec4::new(translation.x, translation.y, translation.z, scale)
        })
        .collect();
    // Send the data to the render world
    commands.insert_resource(EntityData(transforms));
}

// System to update BVH node count in render world settings
fn update_render_world_bvh_count(
    mut settings_query: Query<&mut SDFRenderSettings>,
    bvh_buffer: Option<Res<BVHBuffer>>,
) {
    for mut settings in settings_query.iter_mut() {
        let num_bvh_nodes = bvh_buffer
            .as_ref()
            .map(|buffer| buffer.data.len())
            .unwrap_or(0) as u32;

        settings.num_bvh_nodes = num_bvh_nodes;
    }
}

// System to update BVH node count in main world settings
fn update_bvh_node_count_in_settings(
    mut settings_query: Query<&mut SDFRenderSettings>,
    bvh_data: Option<Res<FlattenedBVH>>,
) {
    for mut settings in settings_query.iter_mut() {
        let num_bvh_nodes = bvh_data.as_ref().map(|data| data.0.len()).unwrap_or(0) as u32;

        settings.num_bvh_nodes = num_bvh_nodes;
    }
}

fn gpu_friendly_f32(f: f32) -> f32 {
    if f == f32::INFINITY {
        return 99999999.;
    } else if f == f32::NEG_INFINITY {
        return -99999999.;
    }
    f
}

// System that runs in the main world to collect transform data
fn build_entity_bvh(mut commands: Commands, entity_data: ResMut<EntityData>) {
    if !entity_data.is_changed() {
        return;
    }

    let entities: Vec<Vec4> = entity_data.into_inner().to_owned().0;
    info!("Building BVH for {} entities", entities.len());

    let mut sdf_entities: Vec<SDFRenderEntity> = entities
        .iter()
        .enumerate()
        .map(|(i, v)| SDFRenderEntity {
            position: Vec3::new(v.x, v.y, v.z),
            scale: v.w,
            node_index: i,
        })
        .collect();

    info!("{:?} sdf entities", sdf_entities);

    let bvh = Bvh::build_par(&mut sdf_entities);

    let flat = bvh.flatten();

    let as_bvh_nodes = flat
        .iter()
        .map(|n| BVHNode {
            min: Vec4::new(
                gpu_friendly_f32(n.aabb.min.x),
                gpu_friendly_f32(n.aabb.min.y),
                gpu_friendly_f32(n.aabb.min.z),
                0.,
            ),
            max: Vec4::new(
                gpu_friendly_f32(n.aabb.max.x),
                gpu_friendly_f32(n.aabb.max.y),
                gpu_friendly_f32(n.aabb.max.z),
                0.,
            ),
            entry_index: n.entry_index,
            exit_index: n.exit_index,
            shape_index: n.shape_index,
            __padding: n.shape_index,
        })
        .collect();

    info!("BVH NODESSSSSSSSS {:?}", as_bvh_nodes);
    commands.insert_resource(FlattenedBVH(as_bvh_nodes));
}

fn update_bvh_buffer(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut bvh_buffer: ResMut<BVHBuffer>,
    flattened_bvh: Res<FlattenedBVH>,
) {
    if !flattened_bvh.is_changed() {
        return;
    }

    let bvh_data = &flattened_bvh.0;

    bvh_buffer.data = bvh_data.clone();
    let byte_size = bvh_buffer.data.len() * std::mem::size_of::<BVHNode>();

    // Debug: Log first few BVH nodes
    for (i, node) in bvh_data.iter().take(50).enumerate() {
        info!(
            "BVH Node {}: type={}, entry={}, exit={}, shape={}, aabb=({:.2},{:.2},{:.2})-({:.2},{:.2},{:.2})",
            i,
            if node.shape_index != u32::MAX { "leaf" } else { "internal" },
            node.entry_index,
            node.exit_index,
            node.shape_index,
            node.min.x, node.min.y, node.min.z,
            node.max.x, node.max.y, node.max.z
        );
    }

    // Create or recreate buffer if needed
    if bvh_buffer.buffer.is_none() || bvh_buffer.capacity < byte_size {
        bvh_buffer.capacity = byte_size.max(1024); // Minimum 1KB
        bvh_buffer.buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("bvh_buffer"),
            size: bvh_buffer.capacity as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        info!(
            "Created BVH buffer with capacity: {} bytes",
            bvh_buffer.capacity
        );
    }

    // Update buffer data
    if let Some(buffer) = &bvh_buffer.buffer {
        if !bvh_buffer.data.is_empty() {
            render_queue.write_buffer(buffer, 0, bytemuck::cast_slice(&bvh_buffer.data));
            info!("Updated BVH buffer with {} BVHnodes", bvh_buffer.data.len());
        }
    }
}

fn sync_entity_positions(
    mut entity_query: Query<(&mut SDFRenderEntity, &GlobalTransform), Changed<GlobalTransform>>,
) {
    for (mut entity, transform) in entity_query.iter_mut() {
        entity.position = transform.translation();
    }
}

// System that runs in the render world to update the buffer
fn update_transform_buffer(
    mut transform_buffer: ResMut<EntityBuffer>,
    transform_data: Option<Res<EntityData>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let Some(data) = transform_data else {
        info!("no data");
        return;
    };

    // Only update if the data has changed
    if !data.is_changed() {
        return;
    }

    info!("Updating entity buffer - {} entities", data.0.len());

    // Update our CPU-side data
    transform_buffer.data = data.0.clone();
    let data_size = transform_buffer.data.len() * std::mem::size_of::<Vec4>();

    // Create or resize buffer if needed
    if transform_buffer.buffer.is_none() || transform_buffer.capacity < data_size {
        info!("resize transform buffer");
        transform_buffer.capacity = (data_size * 2).max(1024); // Buffer with some extra space

        transform_buffer.buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("entity_transform_buffer"),
            size: transform_buffer.capacity as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }

    // Write data to buffer
    if let Some(buffer) = &transform_buffer.buffer {
        if !transform_buffer.data.is_empty() {
            let data_bytes = bytemuck::cast_slice(&transform_buffer.data);
            render_queue.write_buffer(buffer, 0, data_bytes);
        }
    }
}

// System to update entity count in main world settings
fn update_entity_count_in_settings(
    mut settings_query: Query<&mut SDFRenderSettings>,
    transform_data: Option<Res<EntityData>>,
) {
    for mut settings in settings_query.iter_mut() {
        let entity_count = transform_data
            .as_ref()
            .map(|data| data.0.len())
            .unwrap_or(0) as u32;

        settings.entity_count = entity_count;
    }
}

// System to update entity count in render world settings
fn update_render_world_entity_count(
    mut settings_query: Query<&mut SDFRenderSettings>,
    transform_buffer: Option<Res<EntityBuffer>>,
) {
    for mut settings in settings_query.iter_mut() {
        let entity_count = transform_buffer
            .as_ref()
            .map(|buffer| buffer.data.len())
            .unwrap_or(0) as u32;

        // info!("Updating entity count in render world: {} -> {}", settings.entity_count, entity_count);
        settings.entity_count = entity_count;
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct SDFRenderLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct SDFCoarsePrepassLabel;

// The sdf render node used for the render graph
#[derive(Default)]
struct SDFRenderNode;

// The coarse pre-pass render node
#[derive(Default)]
struct SDFCoarsePrepassNode;

// The ViewNode trait is required by the ViewNodeRunner
impl ViewNode for SDFRenderNode {
    // The node needs a query to gather data from the ECS in order to do its rendering,
    // but it's not a normal system so we need to define it manually.
    //
    // This query will only run on the view entity
    type ViewQuery = (
        &'static ViewTarget,
        // prepass textures
        &'static ViewPrepassTextures,
        // This makes sure the node only runs on cameras with the SDFRenderSettings component
        &'static SDFRenderSettings,
        // As there could be multiple sdf render components sent to the GPU (one per camera),
        // we need to get the index of the one that is associated with the current view.
        &'static DynamicUniformIndex<SDFRenderSettings>,
    );

    // Runs the node logic
    // This is where you encode draw commands.
    //
    // This will run on every view on which the graph is running.
    // If you don't want your effect to run on every camera,
    // you'll need to make sure you have a marker component as part of [`ViewQuery`]
    // to identify which camera(s) should run the effect.
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, prepass_textures, _sdf_render_settings, settings_index): QueryItem<
            Self::ViewQuery,
        >,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // Check if sdf rendering is enabled, if not skip the entire pass
        if let Some(enabled_resource) = world.get_resource::<SDFRenderEnabled>() {
            if !enabled_resource.enabled {
                return Ok(());
            }
        }

        // Get the pipeline resource that contains the global data we need
        // to create the render pipeline
        let sdf_render_pipeline = world.resource::<SDFRenderPipeline>();
        let transform_buffer = world.resource::<EntityBuffer>();
        let bvh_buffer = world.resource::<BVHBuffer>();

        // The pipeline cache is a cache of all previously created pipelines.
        // It is required to avoid creating a new pipeline each frame,
        // which is expensive due to shader compilation.
        let pipeline_cache = world.resource::<PipelineCache>();

        // Get the pipeline from the cache
        let Some(pipeline) = pipeline_cache.get_render_pipeline(sdf_render_pipeline.pipeline_id)
        else {
            let pipeline_state =
                pipeline_cache.get_render_pipeline_state(sdf_render_pipeline.pipeline_id);

            match pipeline_state {
                CachedPipelineState::Err(err) => {
                    info!("pipeline err {:?}", err);
                }
                _ => {}
            }
            return Ok(());
        };

        // Get the settings uniform binding
        let settings_uniforms = world.resource::<ComponentUniforms<SDFRenderSettings>>();
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            info!("no settings binding");
            return Ok(());
        };

        let Some(depth_texture) = &prepass_textures.depth else {
            info!("no depth");
            return Ok(());
        };

        // Get transform buffer binding, or create empty buffer if none exists
        let transform_buffer_binding = transform_buffer
            .buffer
            .as_ref()
            .map(|b| b.as_entire_binding());

        // Only create bind group if we have a transform buffer
        let Some(transform_binding) = transform_buffer_binding else {
            info!("no transform binding");
            return Ok(()); // Skip rendering if no transform buffer
        };

        // Get BVH buffer binding, or create empty buffer if none exists
        let bvh_buffer_binding = bvh_buffer.buffer.as_ref().map(|b| b.as_entire_binding());

        let Some(bvh_binding) = bvh_buffer_binding else {
            info!("no bvh binding");
            return Ok(()); // Skip rendering if no BVH buffer
        };

        // This will start a new "sdf render write", obtaining two texture
        // views from the view target - a `source` and a `destination`.
        // `source` is the "current" main texture and you _must_ write into
        // `destination` because calling `post_process_write()` on the
        // [`ViewTarget`] will internally flip the [`ViewTarget`]'s main
        // texture to the `destination` texture. Failing to do so will cause
        // the current main texture information to be lost.
        let post_process = view_target.post_process_write();

        // The bind_group gets created each frame.
        //
        // Normally, you would create a bind_group in the Queue set,
        // but this doesn't work with the post_process_write().
        // The reason it doesn't work is because each post_process_write will alternate the source/destination.
        // The only way to have the correct source/destination for the bind_group
        // is to make sure you get it during the node execution.
        // Get the coarse pass texture
        let Some(coarse_texture) = world.get_resource::<CoarsePassTexture>() else {
            info!("no coarse texture");
            return Ok(());
        };

        let bind_group = render_context.render_device().create_bind_group(
            "sdf_render_bind_group",
            &sdf_render_pipeline.layout,
            &BindGroupEntries::sequential((
                post_process.source,
                &sdf_render_pipeline.sampler,
                &depth_texture.texture.default_view,
                &sdf_render_pipeline.depth_sampler,
                // Coarse pass texture
                &coarse_texture.view,
                // Coarse pass sampler
                &sdf_render_pipeline.coarse_sampler,
            )),
        );

        // Create SDF scene bind group (group 1)
        let sdf_bind_group = render_context.render_device().create_bind_group(
            "sdf_scene_bind_group",
            &sdf_render_pipeline.sdf_layout,
            &BindGroupEntries::sequential((
                // SDF settings uniform (same as main settings)
                settings_binding.clone(),
                // Transform storage buffer
                transform_binding,
                // BVH storage buffer
                bvh_binding,
            )),
        );

        // Begin the render pass
        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("sdf_render_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                // We need to specify the sdf render destination view here
                // to make sure we write to the appropriate texture.
                view: post_process.destination,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // This is mostly just wgpu boilerplate for drawing a fullscreen triangle,
        // using the pipeline/bind_group created above
        render_pass.set_render_pipeline(pipeline);
        // By passing in the index of the sdf render settings on this view, we ensure
        // that in the event that multiple settings were sent to the GPU (as would be the
        // case with multiple cameras), we use the correct one.
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.set_bind_group(1, &sdf_bind_group, &[settings_index.index()]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

impl ViewNode for SDFCoarsePrepassNode {
    type ViewQuery = (
        &'static ViewPrepassTextures,
        &'static SDFRenderSettings,
        &'static DynamicUniformIndex<SDFRenderSettings>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (prepass_textures, _sdf_render_settings, settings_index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // Check if sdf rendering is enabled
        if let Some(enabled_resource) = world.get_resource::<SDFRenderEnabled>() {
            if !enabled_resource.enabled {
                return Ok(());
            }
        }

        let coarse_pipeline = world.resource::<SDFCoarsePrepassPipeline>();
        let transform_buffer = world.resource::<EntityBuffer>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let Some(pipeline) = pipeline_cache.get_render_pipeline(coarse_pipeline.pipeline_id) else {
            return Ok(());
        };

        let settings_uniforms = world.resource::<ComponentUniforms<SDFRenderSettings>>();
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            return Ok(());
        };

        let Some(depth_texture) = &prepass_textures.depth else {
            return Ok(());
        };

        let Some(transform_binding) = transform_buffer
            .buffer
            .as_ref()
            .map(|b| b.as_entire_binding())
        else {
            return Ok(());
        };

        let Some(coarse_texture) = world.get_resource::<CoarsePassTexture>() else {
            return Ok(());
        };

        // Create a dummy screen texture view for the coarse pass
        let dummy_texture = render_context
            .render_device()
            .create_texture(&TextureDescriptor {
                label: Some("dummy_screen_texture"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
        let dummy_view = dummy_texture.create_view(&TextureViewDescriptor::default());

        let bind_group = render_context.render_device().create_bind_group(
            "sdf_coarse_prepass_bind_group",
            &coarse_pipeline.layout,
            &BindGroupEntries::sequential((
                &dummy_view,
                &coarse_pipeline.sampler,
                &depth_texture.texture.default_view,
                &coarse_pipeline.depth_sampler,
            )),
        );

        let sdf_bind_group = render_context.render_device().create_bind_group(
            "sdf_coarse_scene_bind_group",
            &coarse_pipeline.sdf_layout,
            &BindGroupEntries::sequential((settings_binding.clone(), transform_binding)),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("sdf_coarse_prepass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &coarse_texture.view,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_render_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.set_bind_group(1, &sdf_bind_group, &[settings_index.index()]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

// This contains global data used by the render pipeline. This will be created once on startup.
#[derive(Resource)]
struct SDFRenderPipeline {
    layout: BindGroupLayout,
    sdf_layout: BindGroupLayout,
    sampler: Sampler,
    depth_sampler: Sampler,
    coarse_sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for SDFRenderPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        // We need to define the bind group layout used for our pipeline
        let layout = render_device.create_bind_group_layout(
            "sdf_render_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                // The layout entries will only be visible in the fragment stage
                ShaderStages::FRAGMENT,
                (
                    // The screen texture
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    // The sampler that will be used to sample the screen texture
                    sampler(SamplerBindingType::Filtering),
                    // The depth texture
                    texture_2d(TextureSampleType::Depth),
                    // The depth sampler
                    sampler(SamplerBindingType::NonFiltering),
                    // The coarse pass texture
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    // The coarse pass sampler
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );

        // Separate bind group layout for SDF scene data (group 1)
        let sdf_layout = render_device.create_bind_group_layout(
            "sdf_scene_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    // SDF settings uniform
                    uniform_buffer::<SDFRenderSettings>(true),
                    // Storage buffer for entity transforms
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Storage buffer for BVH data
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::FRAGMENT,
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

        // We can create the sampler here since it won't change at runtime and doesn't depend on the view
        let sampler = render_device.create_sampler(&SamplerDescriptor::default());
        let depth_sampler = render_device.create_sampler(&SamplerDescriptor { ..default() });
        let coarse_sampler = render_device.create_sampler(&SamplerDescriptor::default());

        // Get the shader handle
        let shader = world.load_asset(SHADER_ASSET_PATH);

        let pipeline_id = world
            .resource_mut::<PipelineCache>()
            // This will add the pipeline to the cache and queue its creation
            .queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("sdf_render_pipeline".into()),
                layout: vec![layout.clone(), sdf_layout.clone()],
                // This will setup a fullscreen triangle for the vertex state
                vertex: fullscreen_shader_vertex_state(),
                fragment: Some(FragmentState {
                    shader,
                    shader_defs: vec![],
                    // Make sure this matches the entry point of your shader.
                    // It can be anything as long as it matches here and in the shader.
                    entry_point: "fragment".into(),
                    targets: vec![Some(ColorTargetState {
                        format: TextureFormat::bevy_default(),
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                // All of the following properties are not important for this effect so just use the default values.
                // This struct doesn't have the Default trait implemented because not all fields can have a default value.
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            });

        Self {
            layout,
            sdf_layout,
            sampler,
            depth_sampler,
            coarse_sampler,
            pipeline_id,
        }
    }
}

#[derive(Resource)]
struct SDFCoarsePrepassPipeline {
    layout: BindGroupLayout,
    sdf_layout: BindGroupLayout,
    sampler: Sampler,
    depth_sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for SDFCoarsePrepassPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        // Bind group layout for coarse pre-pass (similar to main pass)
        let layout = render_device.create_bind_group_layout(
            "sdf_coarse_prepass_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    // The screen texture
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    // The sampler that will be used to sample the screen texture
                    sampler(SamplerBindingType::Filtering),
                    // The depth texture
                    texture_2d(TextureSampleType::Depth),
                    // The depth sampler
                    sampler(SamplerBindingType::NonFiltering),
                ),
            ),
        );

        // Separate bind group layout for SDF scene data (group 1) - reuse from main pass
        let sdf_layout = render_device.create_bind_group_layout(
            "sdf_coarse_scene_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    // SDF settings uniform
                    uniform_buffer::<SDFRenderSettings>(true),
                    // Storage buffer for entity transforms
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
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

        let sampler = render_device.create_sampler(&SamplerDescriptor::default());
        let depth_sampler = render_device.create_sampler(&SamplerDescriptor { ..default() });

        // Get the coarse pre-pass shader handle
        let shader = world.load_asset("shaders/sdf_coarse_prepass.wgsl");

        let pipeline_id =
            world
                .resource_mut::<PipelineCache>()
                .queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("sdf_coarse_prepass_pipeline".into()),
                    layout: vec![layout.clone(), sdf_layout.clone()],
                    vertex: fullscreen_shader_vertex_state(),
                    fragment: Some(FragmentState {
                        shader,
                        shader_defs: vec![],
                        entry_point: "fragment".into(),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::R32Float,
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                    }),
                    primitive: PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: MultisampleState::default(),
                    push_constant_ranges: vec![],
                    zero_initialize_workgroup_memory: false,
                });

        Self {
            layout,
            sdf_layout,
            sampler,
            depth_sampler,
            pipeline_id,
        }
    }
}

// This is the component that will get passed to the shader
#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub struct SDFRenderSettings {
    pub near_plane: f32,
    pub far_plane: f32,
    pub view_matrix: Mat4,
    pub projection_matrix: Mat4,
    pub camera_position: Vec3,
    pub entity_count: u32,
    pub num_bvh_nodes: u32,
    pub inverse_view_projection: Mat4,
    pub time: f32,
    pub coarse_resolution_factor: f32,
    pub coarse_distance_multiplier: f32,
    pub coarse_max_steps: u32,
}

impl Default for SDFRenderSettings {
    fn default() -> Self {
        Self {
            near_plane: 0.1,
            far_plane: 1000.0,
            view_matrix: Mat4::IDENTITY,
            projection_matrix: Mat4::IDENTITY,
            camera_position: Vec3::ZERO,
            entity_count: 0,
            num_bvh_nodes: 0,
            inverse_view_projection: Mat4::IDENTITY,
            time: 0.0,
            coarse_resolution_factor: 0.0625, // 1/16 resolution
            coarse_distance_multiplier: 16.0, // 25x higher threshold
            coarse_max_steps: 16,             // Reduced steps for performance
        }
    }
}

#[derive(Resource)]
pub struct CoarsePassTexture {
    pub texture: Texture,
    pub view: TextureView,
    pub size: Extent3d,
}

#[derive(Resource, Clone)]
pub struct SDFRenderEnabled {
    pub enabled: bool,
}

impl Default for SDFRenderEnabled {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl ExtractResource for SDFRenderEnabled {
    type Source = SDFRenderEnabled;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

// System to update SDFRenderSettings with current camera data
fn update_camera_settings(
    mut camera_query: Query<(&mut SDFRenderSettings, &GlobalTransform, &Projection), With<Camera>>,
) {
    for (mut settings, global_transform, projection) in camera_query.iter_mut() {
        // Update camera position
        settings.camera_position = global_transform.translation();

        // Update view matrix (inverse of camera's global transform)
        settings.view_matrix = global_transform.compute_matrix().inverse();

        // Update projection matrix
        match projection {
            Projection::Perspective(perspective) => {
                let aspect = perspective.aspect_ratio;
                let fov = perspective.fov;
                let near = perspective.near;
                let far = perspective.far;

                let f = 1.0 / (fov * 0.5).tan();
                settings.projection_matrix = Mat4::from_cols(
                    Vec4::new(f / aspect, 0.0, 0.0, 0.0),
                    Vec4::new(0.0, f, 0.0, 0.0),
                    Vec4::new(0.0, 0.0, (far + near) / (near - far), -1.0),
                    Vec4::new(0.0, 0.0, (2.0 * far * near) / (near - far), 0.0),
                );
            }
            Projection::Orthographic(orthographic) => {
                let left = orthographic.area.min.x;
                let right = orthographic.area.max.x;
                let bottom = orthographic.area.min.y;
                let top = orthographic.area.max.y;
                let near = orthographic.near;
                let far = orthographic.far;

                settings.projection_matrix = Mat4::from_cols(
                    Vec4::new(2.0 / (right - left), 0.0, 0.0, 0.0),
                    Vec4::new(0.0, 2.0 / (top - bottom), 0.0, 0.0),
                    Vec4::new(0.0, 0.0, -2.0 / (far - near), 0.0),
                    Vec4::new(
                        -(right + left) / (right - left),
                        -(top + bottom) / (top - bottom),
                        -(far + near) / (far - near),
                        1.0,
                    ),
                );
            }
            _ => {
                // For custom projections, use identity matrix as fallback
                settings.projection_matrix = Mat4::IDENTITY;
            }
        }

        // Compute and store the inverse view-projection matrix on CPU
        let view_proj = settings.projection_matrix * settings.view_matrix;
        settings.inverse_view_projection = view_proj.inverse();
    }
}

fn update_time_in_settings(
    time: Res<Time>,
    mut camera_query: Query<&mut SDFRenderSettings, With<Camera>>,
) {
    for mut settings in camera_query.iter_mut() {
        settings.time = time.elapsed().as_secs_f32();
    }
}

fn manage_coarse_pass_texture(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    coarse_texture: Option<ResMut<CoarsePassTexture>>,
    camera_query: Query<&SDFRenderSettings, With<Camera>>,
) {
    // Get the first camera's settings to determine texture size
    let Ok(settings) = camera_query.single() else {
        return;
    };

    // Calculate coarse texture size based on resolution factor
    // For now, use a base size - this should be updated based on actual viewport size
    let base_width = 1920u32;
    let base_height = 1080u32;
    let coarse_width = (base_width as f32 * settings.coarse_resolution_factor) as u32;
    let coarse_height = (base_height as f32 * settings.coarse_resolution_factor) as u32;

    let desired_size = Extent3d {
        width: coarse_width.max(1),
        height: coarse_height.max(1),
        depth_or_array_layers: 1,
    };

    // Check if we need to create or recreate the texture
    let needs_update = match &coarse_texture {
        Some(existing) => existing.size != desired_size,
        None => true,
    };

    if needs_update {
        let texture = render_device.create_texture(&TextureDescriptor {
            label: Some("sdf_coarse_pass_texture"),
            size: desired_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&TextureViewDescriptor::default());

        let new_coarse_texture = CoarsePassTexture {
            texture,
            view,
            size: desired_size,
        };

        commands.insert_resource(new_coarse_texture);
    }
}
