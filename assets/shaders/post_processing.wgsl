// This shader computes the chromatic aberration effect

// Since post processing is a fullscreen effect, we use the fullscreen vertex shader provided by bevy.
// This will import a vertex shader that renders a single fullscreen triangle.
//
// A fullscreen triangle is a single triangle that covers the entire screen.
// The box in the top left in that diagram is the screen. The 4 x are the corner of the screen
//
// Y axis
//  1 |  x-----x......
//  0 |  |  s  |  . ´
// -1 |  x_____x´
// -2 |  :  .´
// -3 |  :´
//    +---------------  X axis
//      -1  0  1  2  3
//
// As you can see, the triangle ends up bigger than the screen.
//
// You don't need to worry about this too much since bevy will compute the correct UVs for you.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
struct PostProcessSettings {
    near_plane: f32,
    far_plane: f32,
    view_matrix: mat4x4<f32>,
    projection_matrix: mat4x4<f32>,
    camera_position: vec3<f32>,
    _padding: f32,
    inverse_view_projection: mat4x4<f32>,
}
@group(0) @binding(2) var<uniform> settings: PostProcessSettings;

@group(0) @binding(3) var depth_texture: texture_depth_2d;
@group(0) @binding(4) var depth_sampler: sampler;

// Storage buffer for entity transforms
@group(0) @binding(5) var<storage, read> entity_transforms: array<mat4x4<f32>>;

// Signed distance function for a sphere
fn sphere_sdf(point: vec3<f32>, center: vec3<f32>, radius: f32) -> f32 {
    return length(point - center) - radius;
}

// Transform a point by the inverse of a transformation matrix
fn transform_point(point: vec3<f32>, transform: mat4x4<f32>) -> vec3<f32> {
    // For simplicity, we'll use the inverse transform
    // In a real implementation, you'd want to pass pre-computed inverse matrices
    let world_point = vec4<f32>(point, 1.0);
    let local_point = transform * world_point;
    return local_point.xyz;
}

// Smooth minimum function for blending SDFs
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}


// Scene SDF - returns the minimum distance to any object using smooth blending
fn scene_sdf(point: vec3<f32>) -> f32 {
    var result = 999999.0;
    let transform_count = arrayLength(&entity_transforms);
    let smoothing_factor = 0.5; // Adjust for more/less blending

    for (var i = 0u; i < transform_count; i++) {
        let transform = entity_transforms[i];

        // Transform point to local space of the entity
        // For a unit sphere, we need to account for scale in the transform
        let entity_position = transform[3].xyz;

        // Simple approach: just use position and assume uniform scale
        let scale = length(transform[0].xyz); // Approximate scale from first column
        let dist = sphere_sdf(point, entity_position, scale);

        if (i == 0u) {
            result = dist;
        } else {
            result = smooth_min(result, dist, smoothing_factor);
        }
    }

    return result;
}

// Get ray direction from UV using precomputed inverse view-projection matrix
fn get_ray_direction(uv: vec2<f32>) -> vec3<f32> {
    // Convert UV to NDC (Normalized Device Coordinates)
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    
    // Create points in NDC space (near and far plane)
    let near_point = vec4<f32>(ndc.x, ndc.y, -1.0, 1.0); // Near plane
    let far_point = vec4<f32>(ndc.x, ndc.y, 1.0, 1.0);   // Far plane
    
    // Use precomputed inverse view-projection matrix
    let inv_view_proj = settings.inverse_view_projection;
    
    // Transform points to world space
    let world_near = inv_view_proj * near_point;
    let world_far = inv_view_proj * far_point;
    
    // Perspective divide
    let world_near_3d = world_near.xyz / world_near.w;
    let world_far_3d = world_far.xyz / world_far.w;
    
    // Ray direction is from near to far point
    let ray_dir = normalize(world_far_3d - world_near_3d);
    
    return ray_dir;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Setup ray for raymarching using actual camera parameters
    let uv = in.uv;

    // Ray origin (actual camera position)
    let ray_origin = settings.camera_position;

    // Ray direction using actual camera matrices
    let ray_dir = get_ray_direction(uv);

    // Raymarching parameters
    let max_steps = 64;
    let max_distance = 50.0;
    let surface_threshold = 0.01;

    var ray_pos = ray_origin;
    var total_distance = 0.0;
    var steps_taken = 0;

    // Raymarching loop
    for (var step = 0; step < max_steps; step++) {
        let distance_to_surface = scene_sdf(ray_pos);
        steps_taken = step;

        // If we're close enough to a surface, we've hit something
        if (distance_to_surface < surface_threshold) {
            // Simple lighting calculation using surface normal
            let normal = calculate_normal(ray_pos);
            let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
            let diffuse = max(dot(normal, light_dir), 0.1);
            return vec4<f32>(diffuse, diffuse, diffuse, 1.0);
        }

        // If we've traveled too far, we haven't hit anything
        if (total_distance > max_distance) {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0); // Black background
        }

        // March along the ray
        ray_pos += ray_dir * distance_to_surface;
        total_distance += distance_to_surface;
    }

    // No surface hit - return black
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// Calculate surface normal using finite differences
fn calculate_normal(point: vec3<f32>) -> vec3<f32> {
    let epsilon = 0.001;
    let normal = vec3<f32>(
        scene_sdf(point + vec3<f32>(epsilon, 0.0, 0.0)) - scene_sdf(point - vec3<f32>(epsilon, 0.0, 0.0)),
        scene_sdf(point + vec3<f32>(0.0, epsilon, 0.0)) - scene_sdf(point - vec3<f32>(0.0, epsilon, 0.0)),
        scene_sdf(point + vec3<f32>(0.0, 0.0, epsilon)) - scene_sdf(point - vec3<f32>(0.0, 0.0, epsilon))
    );
    return normalize(normal);
}
