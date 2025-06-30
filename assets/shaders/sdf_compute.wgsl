#import "shaders/sdf_common.wgsl"::{SceneSdfResult, evaluate_scene_sdf}

// Input buffer for query points
@group(0) @binding(0) var<storage, read> query_points: array<vec3<f32>>;

// Output buffer for SDF results
@group(0) @binding(1) var<storage, read_write> sdf_results: array<SceneSdfResult>;

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
    
    // Evaluate SDF using shared implementation
    let result = evaluate_scene_sdf(point);
    
    // Store result
    sdf_results[index] = result;
}