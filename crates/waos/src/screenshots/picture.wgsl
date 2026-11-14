// One picture of a screenshot: a quad over its rectangle, taken to the screenshot's pixels by the
// transforms above it, showing its picture where the picture lies in it, cut by every clip above
// it, each tested in its own coordinates. The pixels are premultiplied, and blended so.

struct Clip {
    // Takes the quad's own coordinates to the clip's.
    to_clip: mat4x4<f32>,
    // Left, top, right, bottom.
    bounds: vec4<f32>,
    // Each corner's horizontal and vertical radius: top-left then top-right, and bottom-right
    // then bottom-left. Zero for a rectangle's.
    top_radii: vec4<f32>,
    bottom_radii: vec4<f32>,
}

struct Quad {
    // Takes the quad's own coordinates to the screenshot's pixels, y down.
    to_target: mat4x4<f32>,
    // Left, top, right, bottom, in the quad's own coordinates.
    bounds: vec4<f32>,
    // Where the picture lies, filling it: left, top, right, bottom.
    picture: vec4<f32>,
    target_size: vec2<f32>,
    clip_count: u32,
    // How many quarter turns clockwise the picture is shown turned, and 1 where it is then
    // mirrored left to right, as a video's frame says.
    quarters: u32,
    flip: u32,
    clips: array<Clip>,
}

@group(0) @binding(0) var<storage, read> quad: Quad;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var picture_sampler: sampler;

struct Corner {
    @builtin(position) position: vec4<f32>,
    // Where the corner is in the quad's own coordinates, which the GPU interpolates across the
    // quad with the transforms' foreshortening.
    @location(0) local: vec2<f32>,
}

@vertex
fn corner(@builtin(vertex_index) index: u32) -> Corner {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let local = mix(quad.bounds.xy, quad.bounds.zw, corners[index]);
    let pixel = quad.to_target * vec4(local, 0.0, 1.0);
    // Pixels, y down, as clip space, y up, keeping `w` for a transform that foreshortens.
    let position = vec4(
        pixel.x * 2.0 / quad.target_size.x - pixel.w,
        pixel.w - pixel.y * 2.0 / quad.target_size.y,
        0.0,
        pixel.w,
    );
    return Corner(position, local);
}

@fragment
fn shade(corner: Corner) -> @location(0) vec4<f32> {
    for (var index = 0u; index < quad.clip_count; index++) {
        if !inside(quad.clips[index], corner.local) {
            discard;
        }
    }
    let at = (corner.local - quad.picture.xy) / (quad.picture.zw - quad.picture.xy);
    if any(at < vec2(0.0)) || any(at > vec2(1.0)) {
        discard;
    }
    // One level, so no derivatives are needed, and none are taken after a discard.
    return textureSampleLevel(picture, picture_sampler, source_of(at), 0.0);
}

// The point of the picture shown at `shown`, a point of where it lies: the mirror undone, then
// the turn.
fn source_of(shown: vec2<f32>) -> vec2<f32> {
    var unflipped = shown;
    if quad.flip == 1u {
        unflipped.x = 1.0 - shown.x;
    }
    switch quad.quarters % 4u {
        case 1u: { return vec2(unflipped.y, 1.0 - unflipped.x); }
        case 2u: { return vec2(1.0 - unflipped.x, 1.0 - unflipped.y); }
        case 3u: { return vec2(1.0 - unflipped.y, unflipped.x); }
        default: { return unflipped; }
    }
}

// Whether `local`, a point of the quad, lies inside `clip`.
fn inside(clip: Clip, local: vec2<f32>) -> bool {
    let mapped = clip.to_clip * vec4(local, 0.0, 1.0);
    let point = mapped.xy / mapped.w;
    let bounds = clip.bounds;
    if any(point < bounds.xy) || any(point > bounds.zw) {
        return false;
    }
    let top_left = clip.top_radii.xy;
    let top_right = clip.top_radii.zw;
    let bottom_right = clip.bottom_radii.xy;
    let bottom_left = clip.bottom_radii.zw;
    return inside_corner(point, bounds.xy + top_left, top_left, vec2(-1.0, -1.0))
        && inside_corner(point, vec2(bounds.z - top_right.x, bounds.y + top_right.y), top_right, vec2(1.0, -1.0))
        && inside_corner(point, bounds.zw - bottom_right, bottom_right, vec2(1.0, 1.0))
        && inside_corner(point, vec2(bounds.x + bottom_left.x, bounds.w - bottom_left.y), bottom_left, vec2(-1.0, 1.0));
}

// Whether `point` lies inside the rounded corner whose ellipse of `radii` is centred at `centre`,
// the corner lying the way `side` points from it; every point not beyond the centre on both axes
// does, as does every point of a corner with no radius.
fn inside_corner(point: vec2<f32>, centre: vec2<f32>, radii: vec2<f32>, side: vec2<f32>) -> bool {
    let beyond = (point - centre) * side;
    if any(radii <= vec2(0.0)) || any(beyond <= vec2(0.0)) {
        return true;
    }
    let scaled = beyond / radii;
    return dot(scaled, scaled) <= 1.0;
}
