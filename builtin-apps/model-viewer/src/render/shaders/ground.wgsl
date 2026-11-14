// The ground: a grid of lines under the model, fading out with distance from its centre.

struct GroundVertex {
    @builtin(position) clip: vec4f,
    @location(0) place: vec2f,
}

// A square around the ground's centre, as a strip of two triangles.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> GroundVertex {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u)) * 2.0 - 1.0;
    let place = globals.ground_centre.xz + corner * globals.ground.z;
    let world = vec4f(place.x, globals.ground.x, place.y, 1.0);
    return GroundVertex(globals.view_projection * world, place);
}

// How much of a pixel a grid's line covers, for a grid of `cell` between its lines.
fn line_cover(place: vec2f, cell: f32) -> f32 {
    let coordinate = place / cell;
    let width = fwidth(coordinate);
    let from_line = abs(fract(coordinate - 0.5) - 0.5) / max(width, vec2f(1e-6));
    let cover = 1.0 - min(min(from_line.x, from_line.y), 1.0);
    // Lines closer together than two pixels would fill the ground, so they fade out instead.
    return cover * (1.0 - smoothstep(0.25, 0.5, max(width.x, width.y)));
}

@fragment
fn fragment(in: GroundVertex) -> @location(0) vec4f {
    let cell = globals.ground.y;
    let cover = max(line_cover(in.place, cell) * 0.5, line_cover(in.place, cell * 5.0));
    let reach = length(in.place - globals.ground_centre.xz) / globals.ground.z;
    let fade = 1.0 - smoothstep(0.2, 1.0, reach);
    return vec4f(to_target(globals.ground_ink.rgb), cover * fade * globals.ground.w);
}
