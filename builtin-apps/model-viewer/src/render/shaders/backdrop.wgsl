// The backdrop: a soft gradient from the top of the picture to the bottom, behind everything.

struct BackdropVertex {
    @builtin(position) clip: vec4f,
    @location(0) height: f32,
}

// One triangle that covers the picture.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> BackdropVertex {
    let corner = vec2f(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    return BackdropVertex(vec4f(corner, 1.0, 1.0), corner.y * 0.5 + 0.5);
}

@fragment
fn fragment(in: BackdropVertex) -> @location(0) vec4f {
    let colour = mix(globals.backdrop_bottom.rgb, globals.backdrop_top.rgb, smoothstep(0.0, 1.0, in.height));
    // A little noise, less than one step of eight bits, so the gradient shows no bands.
    let noise = fract(52.9829189 * fract(dot(in.clip.xy, vec2f(0.06711056, 0.00583715)))) - 0.5;
    return vec4f(to_target(colour + noise / 255.0), 1.0);
}
