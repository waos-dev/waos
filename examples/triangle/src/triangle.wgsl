@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.6),
        vec2<f32>(-0.6, -0.5),
        vec2<f32>(0.6, -0.5),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

// One colour over the whole triangle, so a pixel read back from a frame is known exactly.
@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.95, 0.35, 0.25, 1.0);
}
