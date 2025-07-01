#import "shaders/sdf_common.wgsl"::{SceneSdfResult, raymarch, get_camera_position, default_raymarch_config}

// Input buffer for query points
@group(0) @binding(0) var<storage, read> query_points: array<vec2<f32>>;

struct OnlyDistance {
    distance: f32,
}

// Output buffer for SDF results
@group(0) @binding(1) var<storage, read_write> sdf_results: array<OnlyDistance>;

// Note: SDF scene data (settings and transforms) are now in group 1 via sdf_common.wgsl

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;

    // Check bounds
    if (index >= arrayLength(&query_points)) {
        return;
    }

    // Get the query point
    let point = query_points[index];

    let config = default_raymarch_config();

    // Ray origin (actual camera position)
    let ray_origin = get_camera_position();

    let raymarch_result = raymarch(point, ray_origin, config);

    var result: OnlyDistance;
    result.distance = length(raymarch_result.position - ray_origin);

    // Store result
    sdf_results[index] = result;
}
