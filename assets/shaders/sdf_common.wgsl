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
}

// Settings structure (must match Rust side)
struct SDFRenderSettings {
    near_plane: f32,
    far_plane: f32,
    view_matrix: mat4x4<f32>,
    projection_matrix: mat4x4<f32>,
    camera_position: vec3<f32>,
    entity_count: u32,
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

// BVH traversal to find entities that might intersect with the ray
fn bvh_traverse_for_entities(ray_origin: vec3<f32>, ray_dir: vec3<f32>) -> array<u32, 64> {
    var candidate_entities: array<u32, 64>;
    // Initialize array with invalid indices
    for (var i = 0u; i < 64u; i++) {
        candidate_entities[i] = 0xFFFFFFFFu;
    }
    var candidate_count = 0u;

    // Guard against empty BVH
    if (arrayLength(&bvh_nodes) == 0u) {
        return candidate_entities;
    }

    // Simple stack-based traversal
    var stack: array<u32, 32>;
    var stack_top = 0u;

    // Start with root node (index 0)
    stack[0] = 0u;
    stack_top = 1u;

    while (stack_top > 0u && candidate_count < 64u) {
        stack_top -= 1u;
        let node_index = stack[stack_top];

        let aabb_min = bvh_nodes[node_index].min.xyz;
        let aabb_max = bvh_nodes[node_index].max.xyz;
        let shape_index = bvh_nodes[node_index].shape_index;

        // Test ray against AABB
        if (shape_index < 4294967295) { // u32::MAX
            // Leaf node - add the entity referenced by shape_index
            candidate_entities[candidate_count] = shape_index;
            candidate_count += 1u;
        } else if (ray_aabb_intersect(ray_origin, ray_dir, aabb_min, aabb_max)) {
            // Internal node - add children to stack
            let left_child = bvh_nodes[node_index].entry_index;
            let right_child = bvh_nodes[node_index].exit_index;

            if (stack_top < 30u) {
                stack[stack_top] = left_child;
                stack[stack_top + 1u] = right_child;
                stack_top += 2u;
            }
        }
    }

    return candidate_entities;
}


// BVH traversal to find entities that might intersect with the ray
fn bvh_count_candidates(ray_origin: vec3<f32>, ray_dir: vec3<f32>) -> u32 {
    var candidate_entities: array<u32, 64>;
    // Initialize array with invalid indices
    for (var i = 0u; i < 64u; i++) {
        candidate_entities[i] = 0xFFFFFFFFu;
    }
    var candidate_count = 0u;
    var loop_count = 0u;

    // Guard against empty BVH
    if (arrayLength(&bvh_nodes) == 0u) {
        return 15u;
    }

    // Simple stack-based traversal
    var stack: array<u32, 32>;
    var stack_top = 0u;

    // Start with root node (index 0)
    stack[0] = 0u;
    stack_top = 1u;

    while (stack_top > 0u && candidate_count < 64u) {
        loop_count += 1u;
        stack_top -= 1u;
        let node_index = stack[stack_top];

        let aabb_min = bvh_nodes[node_index].min.xyz;
        let aabb_max = bvh_nodes[node_index].max.xyz;
        let shape_index = bvh_nodes[node_index].shape_index;

        // Test ray against AABB
        if (shape_index < 4294967295) { // u32::MAX
            // Leaf node - add the entity referenced by shape_index
            candidate_entities[candidate_count] = shape_index;
            candidate_count += 1u;
        } else if (ray_aabb_intersect(ray_origin, ray_dir, aabb_min, aabb_max)) {
            // Internal node - add children to stack
            let left_child = bvh_nodes[node_index].entry_index;
            let right_child = bvh_nodes[node_index].exit_index;

            if (stack_top < 30u) {
                stack[stack_top] = left_child;
                stack[stack_top + 1u] = right_child;
                stack_top += 2u;
            }
        }
    }

    return loop_count * 15u;
}

// Initialize a scene SDF result with default values
fn init_scene_sdf_result(point: vec3<f32>, steps: i32) -> SceneSdfResult {
    var result: SceneSdfResult;
    result.distance = 999999.0; // Large initial distance
    result.position = point;
    result.steps = steps;
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

    // result.distance -= abs(sin(sdf_settings.time)) * 0.01;

    return result;
}

// Evaluate SDF at a specific point using BVH acceleration
fn evaluate_scene_sdf_with_bvh(point: vec3<f32>, ray_origin: vec3<f32>, ray_dir: vec3<f32>, steps: i32) -> SceneSdfResult {
    var result = init_scene_sdf_result(point, steps);
    let smoothing_factor = 0.5; // Adjust for more/less blending

    // Fallback to regular evaluation if BVH is empty
    if (arrayLength(&bvh_nodes) == 0u) {
        return evaluate_scene_sdf(point, steps);
    }

    // Use BVH to get candidate entities
    let candidates = bvh_traverse_for_entities(ray_origin, ray_dir);

    var processed_any = false;
    for (var i = 0u; i < 64u; i++) {
        let entity_index = candidates[i];
        // Check if we have a valid entity index
        if (entity_index >= sdf_settings.entity_count) {
            continue;
        }

        let entity = entities[entity_index];

        let sphere_center = entity.xyz;
        let sphere_radius = entity.w;

        // Use reusable combination function from common module
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
    let smoothing_factor = 0.5; // Adjust for more/less blending

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
            smoothing_factor * sphere_radius,
            i == 0u
        );
    }

    return result;
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
    result.distance = total_distance;
    result.position = ray_pos;
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

// BVH-accelerated raymarching from position
fn raymarch_from_position_bvh(start_pos: vec3<f32>, ray_origin: vec3<f32>, ray_dir: vec3<f32>, config: RaymarchConfig) -> SceneSdfResult {
    var ray_pos = start_pos;
    var total_distance = 0.0;

    // Raymarching loop starting from given position with BVH acceleration
    for (var step = 0; step < config.max_steps; step++) {
        let sdf_result = evaluate_scene_sdf_with_bvh(ray_pos, ray_origin, ray_dir, step);

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
