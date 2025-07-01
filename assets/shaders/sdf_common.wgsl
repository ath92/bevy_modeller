// Common SDF types and functions that can be shared between shaders
//
// BIND GROUP STRUCTURE:
// This module defines bind group 1 for SDF scene data that can be shared across shaders:
// - Group 1, Binding 0: PostProcessSettings uniform (camera matrices, entity count, etc.)
// - Group 1, Binding 1: Entity transforms storage buffer (array of vec4: x, y, z, scale)
//
// Shaders that import this module should:
// 1. Use their own bind group 0 for shader-specific resources
// 2. Let this module handle group 1 for shared SDF scene data
// 3. Call the provided functions without passing pointer parameters

// Configuration for raymarching
struct RaymarchConfig {
    max_steps: i32,
    max_distance: f32,
    surface_threshold: f32,
}

// Result of SDF evaluation including distance
struct SceneSdfResult {
    distance: f32,
    position: vec3<f32>,
}

// Settings structure (must match Rust side)
struct PostProcessSettings {
    near_plane: f32,
    far_plane: f32,
    view_matrix: mat4x4<f32>,
    projection_matrix: mat4x4<f32>,
    camera_position: vec3<f32>,
    entity_count: u32,
    inverse_view_projection: mat4x4<f32>,
    time: f32,
}

// Dedicated bind group for SDF scene data (Group 1)
// This allows the common functions to access scene data directly
// without needing pointer parameters, and keeps indexing consistent across shaders
@group(1) @binding(0) var<uniform> sdf_settings: PostProcessSettings;
@group(1) @binding(1) var<storage, read> entity_transforms: array<vec4<f32>>;

// Initialize a scene SDF result with default values
fn init_scene_sdf_result(point: vec3<f32>) -> SceneSdfResult {
    var result: SceneSdfResult;
    result.distance = 999999.0; // Large initial distance
    result.position = point;
    return result;
}

// Default raymarching configuration
fn default_raymarch_config() -> RaymarchConfig {
    var config: RaymarchConfig;
    config.max_steps = 64;
    config.max_distance = 50.0;
    config.surface_threshold = 0.01 + abs(sin(sdf_settings.time)) * 0.05;
    return config;
}

// Extract position from a vec4 transform (x, y, z, scale)
fn extract_position_from_transform(transform: vec4<f32>) -> vec3<f32> {
    return transform.xyz;
}

// Extract scale from a vec4 transform (x, y, z, scale)
fn extract_scale_from_transform(transform: vec4<f32>) -> f32 {
    return transform.w;
}

// SDF for a sphere
fn sphere_sdf(point: vec3<f32>, center: vec3<f32>, radius: f32) -> f32 {
    return length(point - center) - radius;
}

// Smooth minimum operation for blending SDFs
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// Combine a sphere into the existing scene result with smooth blending
fn combine_sphere_into_scene_result(
    current_result: SceneSdfResult,
    point: vec3<f32>,
    sphere_center: vec3<f32>,
    sphere_radius: f32,
    smoothing_factor: f32,
    is_first: bool
) -> SceneSdfResult {
    let sphere_distance = sphere_sdf(point, sphere_center, sphere_radius);

    var result = current_result;

    if (is_first) {
        // First sphere - just use its values
        result.distance = sphere_distance;
    } else {
        // Combine with existing result using smooth minimum
        result.distance = smooth_min(current_result.distance, sphere_distance, smoothing_factor);
    }

    return result;
}

// Evaluate SDF at a specific point using the scene data from the dedicated bind group
fn evaluate_scene_sdf(point: vec3<f32>) -> SceneSdfResult {
    var result = init_scene_sdf_result(point);
    let smoothing_factor = 0.5; // Adjust for more/less blending

    for (var i = 0u; i < sdf_settings.entity_count; i++) {
        let transform = entity_transforms[i];

        // Extract sphere properties using common utilities
        let sphere_center = extract_position_from_transform(transform);
        let sphere_radius = extract_scale_from_transform(transform);

        // Use reusable combination function from common module
        result = combine_sphere_into_scene_result(
            result,
            point,
            sphere_center,
            sphere_radius,
            smoothing_factor,
            i == 0u
        );
    }

    return result;
}

// Calculate surface normal using finite differences
fn calculate_normal(point: vec3<f32>) -> vec3<f32> {
    let epsilon = 0.001;
    let normal = vec3<f32>(
        evaluate_scene_sdf(point + vec3<f32>(epsilon, 0.0, 0.0)).distance -
        evaluate_scene_sdf(point - vec3<f32>(epsilon, 0.0, 0.0)).distance,
        evaluate_scene_sdf(point + vec3<f32>(0.0, epsilon, 0.0)).distance -
        evaluate_scene_sdf(point - vec3<f32>(0.0, epsilon, 0.0)).distance,
        evaluate_scene_sdf(point + vec3<f32>(0.0, 0.0, epsilon)).distance -
        evaluate_scene_sdf(point - vec3<f32>(0.0, 0.0, epsilon)).distance
    );
    return normalize(normal);
}

// Get ray direction from UV using precomputed inverse view-projection matrix
fn get_ray_direction(uv: vec2<f32>, inverse_view_projection: mat4x4<f32>) -> vec3<f32> {
    // Convert UV to NDC (Normalized Device Coordinates)
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);

    // Create points in NDC space (near and far plane)
    let near_point = vec4<f32>(ndc.x, ndc.y, -1.0, 1.0); // Near plane
    let far_point = vec4<f32>(ndc.x, ndc.y, 1.0, 1.0);   // Far plane

    // Transform points to world space
    let world_near = inverse_view_projection * near_point;
    let world_far = inverse_view_projection * far_point;

    // Perspective divide
    let world_near_3d = world_near.xyz / world_near.w;
    let world_far_3d = world_far.xyz / world_far.w;

    // Ray direction is from near to far point
    let ray_dir = normalize(world_far_3d - world_near_3d);

    return ray_dir;
}

// Convenience functions to access SDF settings without exposing the internal binding
// These provide a clean API for shaders using this common module

// Get camera position from the SDF settings
fn get_camera_position() -> vec3<f32> {
    return sdf_settings.camera_position;
}

// Get inverse view projection matrix from SDF settings
fn get_inverse_view_projection() -> mat4x4<f32> {
    return sdf_settings.inverse_view_projection;
}

fn raymarch(uv: vec2<f32>, ray_origin: vec3<f32>, config: RaymarchConfig) -> SceneSdfResult {
    // Ray direction using actual camera matrices
    let ray_dir = get_ray_direction(uv, get_inverse_view_projection());

    var ray_pos = ray_origin;
    var total_distance = 0.0;

    // Raymarching loop
    for (var step = 0; step < config.max_steps; step++) {
        let sdf_result = evaluate_scene_sdf(ray_pos);

        // If we're close enough to a surface, we've hit something
        if (sdf_result.distance < config.surface_threshold) {
            return sdf_result;
        }

        // If we've traveled too far, we haven't hit anything
        if (total_distance > config.max_distance) {
            break;
        }

        // March along the ray
        ray_pos += ray_dir * sdf_result.distance;
        total_distance += sdf_result.distance;
    }

    var result: SceneSdfResult;
    result.distance = config.max_distance;
    result.position = ray_pos;
    return result;
}
