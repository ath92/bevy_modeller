#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import "shaders/sdf_common.wgsl"::{PostProcessSettings, SceneSdfResult, RaymarchConfig, default_raymarch_config, evaluate_scene_sdf, calculate_normal, get_ray_direction, get_camera_position, get_inverse_view_projection, get_entity_count}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: PostProcessSettings;

@group(0) @binding(3) var depth_texture: texture_depth_2d;
@group(0) @binding(4) var depth_sampler: sampler;

// Note: SDF scene data (settings and transforms) are now in group 1 via sdf_common.wgsl

// Scene SDF - now uses the common implementation directly
fn scene_sdf(point: vec3<f32>) -> SceneSdfResult {
    return evaluate_scene_sdf(point);
}

// Calculate surface normal using shared implementation
fn scene_normal(point: vec3<f32>) -> vec3<f32> {
    return calculate_normal(point);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Setup ray for raymarching using actual camera parameters
    let uv = in.uv;

    // Ray origin (actual camera position)
    let ray_origin = get_camera_position();

    // Ray direction using actual camera matrices
    let ray_dir = get_ray_direction(uv, get_inverse_view_projection());

    // Use improved raymarching with configuration
    let config = default_raymarch_config();

    var ray_pos = ray_origin;
    var total_distance = 0.0;

    // Raymarching loop
    for (var step = 0; step < config.max_steps; step++) {
        let sdf_result = scene_sdf(ray_pos);

        // If we're close enough to a surface, we've hit something
        if (sdf_result.distance < config.surface_threshold) {
            // Simple lighting calculation using surface normal
            let normal = scene_normal(ray_pos);
            let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
            let diffuse = max(dot(normal, light_dir), 0.1);

            // Color based on material ID for variety
            let material_factor = f32(sdf_result.material_id % 3u) / 3.0;
            let color_tint = vec3<f32>(0.5 + material_factor * 0.5, diffuse, 0.5 + (1.0 - material_factor) * 0.5);
            return vec4<f32>(color_tint * diffuse, 1.0);
        }

        // If we've traveled too far, we haven't hit anything
        if (total_distance > config.max_distance) {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0); // Black background
        }

        // March along the ray
        ray_pos += ray_dir * sdf_result.distance;
        total_distance += sdf_result.distance;
    }

    // No surface hit - return black
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}