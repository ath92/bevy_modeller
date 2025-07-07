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
    steps: i32,
    normal: vec3<f32>,
}

// Settings structure (must match Rust side)
struct SDFRenderSettings {
    near_plane: f32,
    far_plane: f32,
    view_matrix: mat4x4<f32>,
    projection_matrix: mat4x4<f32>,
    camera_position: vec3<f32>,
    entity_count: u32,
    num_bvh_nodes: u32,
    inverse_view_projection: mat4x4<f32>,
    time: f32,
    coarse_resolution_factor: f32,
    coarse_distance_multiplier: f32,
    coarse_max_steps: u32,
}

struct BVHNode {
    min: vec4<f32>,
    max: vec4<f32>,
    entry_index: u32,
    exit_index: u32,
    shape_index: u32,
    _padding: u32,
}

// Dedicated bind group for SDF scene data (Group 1)
// This allows the common functions to access scene data directly
// without needing pointer parameters, and keeps indexing consistent across shaders
@group(1) @binding(0) var<uniform> sdf_settings: SDFRenderSettings;
@group(1) @binding(1) var<storage, read> entities: array<vec4<f32>>;
@group(1) @binding(2) var<storage, read> bvh_nodes: array<BVHNode>;



// Initialize a scene SDF result with default values
fn init_scene_sdf_result(point: vec3<f32>, steps: i32) -> SceneSdfResult {
    var result: SceneSdfResult;
    result.distance = 999999.0; // Large initial distance
    result.position = point;
    result.steps = steps;
    result.normal = vec3<f32>(0.0, 0.0, 0.0);
    return result;
}

// Default raymarching configuration
fn default_raymarch_config() -> RaymarchConfig {
    var config: RaymarchConfig;
    config.max_steps = 48;
    config.max_distance = 50.0;
    config.surface_threshold = 0.01;
    return config;
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

fn circular_geometrical_smin(a: f32, b: f32, k: f32) -> f32 {
    let k_adjusted = k * (1.0 / (1.0 - sqrt(0.5)));
    return max(k_adjusted, min(a, b)) -
           length(max(vec2<f32>(k_adjusted) - vec2<f32>(a, b), vec2<f32>(0.0)));
}

fn quadratic_smin( a: f32, b: f32, k: f32 ) -> f32
{
    let k4 = k* 4.0;
    let h = max( k4-abs(a-b), 0.0 )/k4;
    return min(a,b) - h*h*k4*(1.0/4.0);
}


// Calculate surface normal using finite differences
fn calculate_normal(point: vec3<f32>) -> vec3<f32> {
    let epsilon = 0.001;
    let normal = vec3<f32>(
        evaluate_scene_sdf(point + vec3<f32>(epsilon, 0.0, 0.0), 0).distance -
        evaluate_scene_sdf(point - vec3<f32>(epsilon, 0.0, 0.0), 0).distance,
        evaluate_scene_sdf(point + vec3<f32>(0.0, epsilon, 0.0), 0).distance -
        evaluate_scene_sdf(point - vec3<f32>(0.0, epsilon, 0.0), 0).distance,
        evaluate_scene_sdf(point + vec3<f32>(0.0, 0.0, epsilon), 0).distance -
        evaluate_scene_sdf(point - vec3<f32>(0.0, 0.0, epsilon), 0).distance
    );
    return normalize(normal);
}

// Calculate surface normal using finite differences with BVH acceleration
fn calculate_normal_bvh(point: vec3<f32>, candidates: ptr<function, array<u32, 32>>) -> vec3<f32> {
    let epsilon = 0.001;
    let normal = vec3<f32>(
        evaluate_scene_sdf_with_bvh(point + vec3<f32>(epsilon, 0.0, 0.0), candidates, 0).distance -
        evaluate_scene_sdf_with_bvh(point - vec3<f32>(epsilon, 0.0, 0.0), candidates, 0).distance,
        evaluate_scene_sdf_with_bvh(point + vec3<f32>(0.0, epsilon, 0.0), candidates, 0).distance -
        evaluate_scene_sdf_with_bvh(point - vec3<f32>(0.0, epsilon, 0.0), candidates, 0).distance,
        evaluate_scene_sdf_with_bvh(point + vec3<f32>(0.0, 0.0, epsilon), candidates, 0).distance -
        evaluate_scene_sdf_with_bvh(point - vec3<f32>(0.0, 0.0, epsilon), candidates, 0).distance
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

// Get coarse pass settings
fn get_coarse_max_steps() -> u32 {
    return sdf_settings.coarse_max_steps;
}

fn get_coarse_distance_multiplier() -> f32 {
    return sdf_settings.coarse_distance_multiplier;
}


// Ray-AABB intersection test
fn ray_aabb_intersect(ray_origin: vec3<f32>, ray_dir: vec3<f32>, aabb_min: vec3<f32>, aabb_max: vec3<f32>) -> bool {
    let inv_dir = 1.0 / ray_dir;
    let t_min = (aabb_min - ray_origin) * inv_dir;
    let t_max = (aabb_max - ray_origin) * inv_dir;

    let t1 = min(t_min, t_max);
    let t2 = max(t_min, t_max);

    let t_near = max(max(t1.x, t1.y), t1.z);
    let t_far = min(min(t2.x, t2.y), t2.z);

    return t_near <= t_far && t_far >= 0.0;
}

// Linear BVH traversal following the reference implementation
fn bvh_traverse_for_entities(ray_origin: vec3<f32>, ray_dir: vec3<f32>) -> array<u32, 32> {
    var candidate_entities: array<u32, 32>;
    // Initialize array with invalid indices
    for (var i = 0u; i < 32u; i++) {
        candidate_entities[i] = 0xFFFFFFFFu;
    }
    var candidate_count = 0u;

    var index = 0u;
    let max_length = sdf_settings.num_bvh_nodes;

    // Iterate while the node index is valid
    while (index < max_length && candidate_count < 32u) {
        let node = bvh_nodes[index];
        let shape_index = node.shape_index;

        if (shape_index < 0xFFFFFFFFu) { // u32::MAX - leaf node
            // Add the entity to candidates
            candidate_entities[candidate_count] = shape_index;
            candidate_count += 1u;

            // Exit the current node
            index = node.exit_index;
        } else if (ray_aabb_intersect(ray_origin, ray_dir, node.min.xyz, node.max.xyz)) {
            // If AABB test passes, proceed to entry_index (go down the BVH branch)
            index = node.entry_index;
        } else {
            // If AABB test fails, proceed to exit_index (skip this subtree)
            index = node.exit_index;
        }
    }

    return candidate_entities;
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
        result.distance = quadratic_smin(current_result.distance, sphere_distance, smoothing_factor);
    }

    return result;
}

// Evaluate SDF at a specific point using BVH acceleration
fn evaluate_scene_sdf_with_bvh(point: vec3<f32>, candidates: ptr<function, array<u32, 32>>, steps: i32) -> SceneSdfResult {
    var result = init_scene_sdf_result(point, steps);
    let smoothing_factor = 0.5; // Adjust for more/less blending

    var processed_any = false;
    for (var i = 0u; i < 32u; i++) {
        let entity_index = (*candidates)[i];
        // Check if we have a valid entity index
        if (entity_index >= sdf_settings.entity_count) {
            continue;
        }

        let entity = entities[entity_index];

        let sphere_center = entity.xyz;
        let sphere_radius = entity.w;

        result = combine_sphere_into_scene_result(
            result,
            point,
            sphere_center,
            sphere_radius,
            smoothing_factor * sphere_radius,
            !processed_any
        );

        processed_any = true;
    }
    return result;
}

// Evaluate SDF at a specific point using the scene data from the dedicated bind group
fn evaluate_scene_sdf(point: vec3<f32>, steps: i32) -> SceneSdfResult {
    var result = init_scene_sdf_result(point, steps);
    let smoothing_factor = 0.1; // Adjust for more/less blending

    for (var i = 0u; i < sdf_settings.entity_count; i++) {
        let entity = entities[i];

        // Extract sphere properties using common utilities
        let sphere_center = entity.xyz;
        let sphere_radius = entity.w;

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

fn raymarch(uv: vec2<f32>, ray_origin: vec3<f32>, config: RaymarchConfig) -> SceneSdfResult {
    // Ray direction using actual camera matrices
    let ray_dir = get_ray_direction(uv, sdf_settings.inverse_view_projection);

    var ray_pos = ray_origin;
    var total_distance = 0.001;

    // Raymarching loop
    for (var step = 0; step < config.max_steps; step++) {
        let sdf_result = evaluate_scene_sdf(ray_pos, step);

        // If we're close enough to a surface, we've hit something
        if (sdf_result.distance < config.surface_threshold) {
            // Calculate normal at the surface point
            var result = sdf_result;
            result.normal = calculate_normal(ray_pos);
            return result;
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
    result.distance = total_distance;
    result.position = ray_pos;
    result.normal = vec3<f32>(0.0, 0.0, 0.0);
    return result;
}

fn raymarch_from_position(start_pos: vec3<f32>, ray_dir: vec3<f32>, config: RaymarchConfig) -> SceneSdfResult {
    var ray_pos = start_pos;
    var total_distance = 0.0;

    // Raymarching loop starting from given position
    for (var step = 0; step < config.max_steps; step++) {
        let sdf_result = evaluate_scene_sdf(ray_pos, step);

        // If we're close enough to a surface, we've hit something
        if (sdf_result.distance < config.surface_threshold) {
            // Calculate normal at the surface point
            var result = sdf_result;
            result.normal = calculate_normal(ray_pos);
            return result;
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
    result.normal = vec3<f32>(0.0, 0.0, 0.0);
    return result;
}

// BVH-accelerated raymarching from position
fn raymarch_from_position_bvh(start_pos: vec3<f32>, ray_dir: vec3<f32>, config: RaymarchConfig) -> SceneSdfResult {
    var ray_pos = start_pos;
    var total_distance = 0.0;

    // Use BVH to get candidate entities
    // let candidates = bvh_traverse_regarded();
    var candidates = bvh_traverse_for_entities(start_pos, ray_dir);
    // Raymarching loop starting from given position with BVH acceleration
    for (var step = 0; step < config.max_steps; step++) {
        // let sdf_result = evaluate_scene_sdf(ray_pos, step);
        let sdf_result = evaluate_scene_sdf_with_bvh(ray_pos, &candidates, step);

        // If we're close enough to a surface, we've hit something
        if (sdf_result.distance < config.surface_threshold) {
            // Calculate normal using the same candidate list for consistency
            var result = sdf_result;
            result.normal = calculate_normal_bvh(ray_pos, &candidates);
            return result;
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
    result.normal = vec3<f32>(0.0, 0.0, 0.0);
    return result;
}
