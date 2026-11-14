// A picture drawn through a look, over the whole target, shown as it should be seen: turned
// clockwise by its rotation, then mirrored left to right where it is flipped, as a camera's frame
// says; softened by its own blur, its white balance shifted, its colour kept or drained, its
// contrast curved and its corners darkened, in that order. The numbers are `looks.rs`'s `Grade`,
// laid out by `look_painter.rs`.

struct Grade {
    // What red, green and blue are multiplied by, then how much colour is kept.
    tint: vec4<f32>,
    // The contrast, the softness, the vignette, and 1 where the picture is flipped left to right
    // after its rotation.
    tone: vec4<f32>,
    // The picture's width over its height, and how many quarter turns clockwise it is shown
    // turned; the rest is unused.
    frame: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> grade: Grade;

// How many points of the picture a blurred point is the mean of, spread over a disc.
const TAPS: i32 = 24;
// The turn between one point of the disc and the next, which spreads them evenly.
const GOLDEN_ANGLE: f32 = 2.39996323;
// The disc's radius, as a share of the picture's height.
const BLUR_RADIUS: f32 = 0.014;
// How much each of red, green and blue counts in a colour's lightness: Rec. 709's.
const LIGHTNESS: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle that covers the target, the picture's top-left at the target's.
@vertex
fn corner(@builtin(vertex_index) index: u32) -> Corner {
    let across = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Corner;
    out.position = vec4<f32>(across * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(across.x, 1.0 - across.y);
    return out;
}

fn blurred(uv: vec2<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var tap = 0; tap < TAPS; tap++) {
        let turn = f32(tap) * GOLDEN_ANGLE;
        let reach = BLUR_RADIUS * sqrt((f32(tap) + 0.5) / f32(TAPS));
        let offset = vec2<f32>(cos(turn) / grade.frame.x, sin(turn)) * reach;
        sum += textureSampleLevel(picture, picture_sampler, uv + offset, 0.0).rgb;
    }
    return sum / f32(TAPS);
}

// The point of the picture shown at `shown`, a point of the target: the flip undone, then the
// turn.
fn source_of(shown: vec2<f32>) -> vec2<f32> {
    let unflipped = vec2<f32>(mix(shown.x, 1.0 - shown.x, grade.tone.w), shown.y);
    let quarters = u32(grade.frame.y + 0.5) % 4u;
    if quarters == 1u {
        return vec2<f32>(unflipped.y, 1.0 - unflipped.x);
    }
    if quarters == 2u {
        return vec2<f32>(1.0 - unflipped.x, 1.0 - unflipped.y);
    }
    if quarters == 3u {
        return vec2<f32>(1.0 - unflipped.y, unflipped.x);
    }
    return unflipped;
}

@fragment
fn graded(corner: Corner) -> @location(0) vec4<f32> {
    let uv = source_of(corner.uv);
    var color = textureSampleLevel(picture, picture_sampler, uv, 0.0).rgb;
    if grade.tone.y > 0.0 {
        color = mix(color, blurred(uv), grade.tone.y);
    }
    color = clamp(color * grade.tint.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    color = mix(vec3<f32>(dot(color, LIGHTNESS)), color, grade.tint.a);
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    let curved = color * color * (3.0 - 2.0 * color);
    color = mix(color, curved, grade.tone.x);
    let from_middle = length((corner.uv - 0.5) * 2.0);
    color *= 1.0 - grade.tone.z * smoothstep(0.5, 1.4, from_middle);
    return vec4<f32>(color, 1.0);
}
