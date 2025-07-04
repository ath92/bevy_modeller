#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import "shaders/sdf_common.wgsl"::{PostProcessSettings, SceneSdfResult, RaymarchConfig, evaluate_scene_sdf, get_camera_position, get_ray_direction, get_inverse_view_projection, get_coarse_max_steps, get_coarse_distance_multiplier, raymarch}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var depth_sampler: sampler;

// Coarse raymarching configuration with dynamic settings from uniform buffer
fn coarse_raymarch_config() -> RaymarchConfig {
    var config: RaymarchConfig;
    config.max_steps = i32(get_coarse_max_steps());  // Dynamic step count from settings
    config.max_distance = 50.0;         // Same max distance as main pass
    config.surface_threshold = 0.01 * get_coarse_distance_multiplier();  // Dynamic threshold
    return config;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Setup ray for coarse raymarching
    let uv = in.uv;
    let config = coarse_raymarch_config();

    // Ray origin (actual camera position)
    let ray_origin = get_camera_position();

    // Perform coarse raymarching
    let result = raymarch(uv, ray_origin, config);

    // Output the distance in the red channel
    // This will be used by the main pass to start raymarching
    return vec4<f32>(result.distance, 0.0, 0.0, 1.0);
}
