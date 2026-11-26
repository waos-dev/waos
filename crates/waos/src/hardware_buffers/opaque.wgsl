// One triangle over the whole target, whose every pixel is written alpha 1: the pipeline writes
// alpha alone, so the colour drawn stays.

@vertex
fn corner(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn opaque() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}
