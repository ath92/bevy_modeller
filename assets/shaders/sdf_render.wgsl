#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import "shaders/sdf_common.wgsl"::{PostProcessSettings, SceneSdfResult, RaymarchConfig, default_raymarch_config, calculate_normal, raymarch, get_camera_position}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var depth_sampler: sampler;


@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Setup ray for raymarching using actual camera parameters
    let uv = in.uv;

    let config = default_raymarch_config();

    // Ray origin (actual camera position)
    let ray_origin = get_camera_position();

    let result = raymarch(uv, ray_origin, config);

    if (result.distance < config.max_distance) {
        // Simple lighting calculation using surface normal
        let normal = calculate_normal(result.position);
        let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
        let diffuse = max(dot(normal, light_dir), 0.1);

        return vec4<f32>(diffuse, diffuse, diffuse, 1.0);
    }

    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
