#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import "shaders/sdf_common.wgsl"::{PostProcessSettings, SceneSdfResult, RaymarchConfig, default_raymarch_config, raymarch, get_camera_position, get_ray_direction, get_inverse_view_projection, raymarch_from_position, raymarch_from_position_bvh}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var depth_sampler: sampler;

@group(0) @binding(4) var coarse_pass_texture: texture_2d<f32>;
@group(0) @binding(5) var coarse_pass_sampler: sampler;


@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Setup ray for raymarching using actual camera parameters
    let uv = in.uv;

    // Sample coarse pass result
    let coarse_distance = textureSample(coarse_pass_texture, coarse_pass_sampler, uv).r;

    let config = default_raymarch_config();

    // Early termination: if coarse pass found nothing, return immediately
    if (coarse_distance >= config.max_distance) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    // Ray origin (actual camera position)
    let ray_origin = get_camera_position();
    let ray_dir = get_ray_direction(uv, get_inverse_view_projection());

    // Start raymarching from coarse distance
    let start_pos = ray_origin + ray_dir * (coarse_distance);

    // Perform fine raymarching starting from the coarse position with BVH acceleration
    let result = raymarch_from_position_bvh(start_pos, ray_dir, config);

    if (result.distance < config.max_distance) {
        // Simple lighting calculation using surface normal from raymarch result
        let normal = result.normal;
        let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
        let diffuse = max(dot(normal, light_dir), 0.1);

        return vec4<f32>(diffuse, diffuse, diffuse, 1.0);
    }

    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
