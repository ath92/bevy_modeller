#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var depth_sampler: sampler;

struct DepthPostProcessSettings {
    near_plane: f32,
    far_plane: f32,
    intensity: f32,
    _padding: f32,
}

@group(0) @binding(4) var<uniform> settings: DepthPostProcessSettings;

fn linearize_depth(depth: f32, near: f32, far: f32) -> f32 {
    let z = depth * 2.0 - 1.0; // Convert to NDC
    return (2.0 * near * far) / (far + near - z * (far - near));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Sample the depth value
    let depth = textureSample(depth_texture, depth_sampler, in.uv);

    // Linearize the depth for better visualization
    let linear_depth = linearize_depth(depth, settings.near_plane, settings.far_plane);

    // Normalize the linear depth to 0-1 range for visualization
    let normalized_depth = linear_depth / settings.far_plane;

    // Apply intensity and clamp to valid range
    let final_depth = clamp(normalized_depth * settings.intensity, 0.0, 1.0);

    // Convert to grayscale by setting all RGB channels to the same value
    return vec4<f32>(1.0, final_depth, final_depth, 1.0);
}
