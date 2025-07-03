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

/// This example uses a shader source file from the assets subdirectory
const SHADER_ASSET_PATH: &str = "shaders/sdf_render.wgsl";

// Resource to hold transform data in the render world
#[derive(Resource)]
pub struct EntityBuffer {
    pub buffer: Option<Buffer>,
    pub data: Vec<Vec4>,
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

// Component to mark entities whose transforms should be sent to the shader
#[derive(Component)]
pub struct SDFRenderEntity {
    pub index: u32,
    pub position: Vec3,
    pub scale: f32,
}

// Resource to transfer data from main world to render world
#[derive(Resource, Clone)]
struct EntityData(Vec<Vec4>);

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
            // It's important to derive [`ExtractComponent`] on [`PostProcessingSettings`]
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
        ))
        // Initialize the PostProcessEnabled resource
        .init_resource::<SDFRenderEnabled>()
        // Add the system to collect transform data
        .add_systems(
            Update,
            (
                sync_entity_positions,
                collect_entity_data,
                update_camera_settings,
                update_entity_count_in_settings,
                update_time_in_settings,
            ),
        );

        // We need to get the render app from the main app
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<EntityBuffer>()
            .add_systems(
                Render,
                (
                    update_transform_buffer.in_set(RenderSet::PrepareResources),
                    update_render_world_entity_count
                        .in_set(RenderSet::PrepareResources)
                        .after(update_transform_buffer),
                ),
            )
            // Bevy's renderer uses a render graph which is a collection of nodes in a directed acyclic graph.
            // It currently runs on each view/camera and executes each node in the specified order.
            // It will make sure that any node that needs a dependency from another node
            // only runs when that dependency is done.
            //
            // Each node can execute arbitrary work, but it generally runs at least one render pass.
            // A node only has access to the render world, so if you need data from the main world
            // you need to extract it manually or with the plugin like above.
            // Add a [`Node`] to the [`RenderGraph`]
            // The Node needs to impl FromWorld
            //
            // The [`ViewNodeRunner`] is a special [`Node`] that will automatically run the node for each view
            // matching the [`ViewQuery`]
            .add_render_graph_node::<ViewNodeRunner<SDFRenderNode>>(
                // Specify the label of the graph, in this case we want the graph for 3d
                Core3d,
                // It also needs the label of the node
                SDFRenderLabel,
            )
            .add_render_graph_edges(
                Core3d,
                // Specify the node ordering.
                // This will automatically create all required node edges to enforce the given ordering.
                (
                    Node3d::Tonemapping,
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
            // Initialize the pipeline
            .init_resource::<SDFRenderPipeline>();
    }
}

// System that runs in the main world to collect transform data
fn collect_entity_data(entity_query: Query<&SDFRenderEntity>, mut commands: Commands) {
    let mut entities: Vec<&SDFRenderEntity> = entity_query.iter().collect();
    entities.sort_by_key(|e| e.index);

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

// The sdf render node used for the render graph
#[derive(Default)]
struct SDFRenderNode;

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
        let bind_group = render_context.render_device().create_bind_group(
            "sdf_render_bind_group",
            &sdf_render_pipeline.layout,
            // It's important for this to match the BindGroupLayout defined in the PostProcessPipeline
            &BindGroupEntries::sequential((
                // Make sure to use the source view
                post_process.source,
                // Use the sampler created for the pipeline
                &sdf_render_pipeline.sampler,
                // Depth
                &depth_texture.texture.default_view,
                // Depth sampler
                &sdf_render_pipeline.depth_sampler,
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

// This contains global data used by the render pipeline. This will be created once on startup.
#[derive(Resource)]
struct SDFRenderPipeline {
    layout: BindGroupLayout,
    sdf_layout: BindGroupLayout,
    sampler: Sampler,
    depth_sampler: Sampler,
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
                ),
            ),
        );

        // We can create the sampler here since it won't change at runtime and doesn't depend on the view
        let sampler = render_device.create_sampler(&SamplerDescriptor::default());
        let depth_sampler = render_device.create_sampler(&SamplerDescriptor { ..default() });

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
            pipeline_id,
        }
    }
}

// This is the component that will get passed to the shader
#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct SDFRenderSettings {
    pub near_plane: f32,
    pub far_plane: f32,
    pub view_matrix: Mat4,
    pub projection_matrix: Mat4,
    pub camera_position: Vec3,
    pub entity_count: u32,
    pub inverse_view_projection: Mat4,
    pub time: f32,
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
