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
        render_graph::{
            NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
        RenderApp,
    },
};

const DEPTH_SHADER_ASSET_PATH: &str = "shaders/depth_post_process.wgsl";

pub struct DepthPostProcessPlugin;

impl Plugin for DepthPostProcessPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<DepthPostProcessSettings>::default(),
            UniformComponentPlugin::<DepthPostProcessSettings>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .add_render_graph_node::<ViewNodeRunner<DepthPostProcessNode>>(
                Core3d,
                DepthPostProcessLabel,
            )
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::Tonemapping,
                    DepthPostProcessLabel,
                    Node3d::EndMainPassPostProcessing,
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<DepthPostProcessPipeline>();
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct DepthPostProcessLabel;

#[derive(Default)]
struct DepthPostProcessNode;

impl ViewNode for DepthPostProcessNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewPrepassTextures,
        &'static DepthPostProcessSettings,
        &'static DynamicUniformIndex<DepthPostProcessSettings>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, prepass_textures, _settings, settings_index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline = world.resource::<DepthPostProcessPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline.pipeline_id) else {
            let pipeline_state = pipeline_cache.get_render_pipeline_state(pipeline.pipeline_id);

            match pipeline_state {
                CachedPipelineState::Err(err) => {
                    info!("pipeline err {:?}", err);
                }
                _ => {}
            }
            return Ok(());
        };

        let settings_uniforms = world.resource::<ComponentUniforms<DepthPostProcessSettings>>();
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();

        let Some(depth_texture) = &prepass_textures.depth else {
            info!("no depth");
            return Ok(());
        };

        let bind_group = render_context.render_device().create_bind_group(
            "depth_post_process_bind_group",
            &pipeline.layout,
            &BindGroupEntries::sequential((
                &depth_texture.texture.default_view,
                &pipeline.depth_sampler,
                settings_binding.clone(),
            )),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("depth_post_process_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_render_pipeline(render_pipeline);
        render_pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
        render_pass.draw(0..3, 0..1);

        info!("got em");
        Ok(())
    }
}

#[derive(Resource)]
struct DepthPostProcessPipeline {
    layout: BindGroupLayout,
    depth_sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for DepthPostProcessPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

        let layout = render_device.create_bind_group_layout(
            "depth_post_process_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    // Depth texture
                    texture_2d(TextureSampleType::Depth),
                    // Depth sampler
                    sampler(SamplerBindingType::NonFiltering),
                    // Settings uniform
                    uniform_buffer::<DepthPostProcessSettings>(true),
                ),
            ),
        );

        let depth_sampler = render_device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            ..default()
        });

        let shader = world.load_asset(DEPTH_SHADER_ASSET_PATH);

        let pipeline_id =
            world
                .resource_mut::<PipelineCache>()
                .queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("depth_post_process_pipeline".into()),
                    layout: vec![layout.clone()],
                    vertex: fullscreen_shader_vertex_state(),
                    fragment: Some(FragmentState {
                        shader,
                        shader_defs: vec![],
                        entry_point: "fragment".into(),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::bevy_default(),
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
            depth_sampler,
            pipeline_id,
        }
    }
}

#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct DepthPostProcessSettings {
    pub near_plane: f32,
    pub far_plane: f32,
    pub intensity: f32,
}
