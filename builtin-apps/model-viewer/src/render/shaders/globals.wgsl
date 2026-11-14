// What every shader of the scene reads: the camera, the light and how the scene is shown.
// Laid out as `render::globals::Globals` writes it.
struct Globals {
    view_projection: mat4x4f,
    // The camera's place, and the exposure in w.
    camera: vec4f,
    // The direction towards the light, and its strength in w: zero when the light is off.
    light: vec4f,
    // The backdrop's colours at the top and the bottom of the picture, as displayed.
    backdrop_top: vec4f,
    backdrop_bottom: vec4f,
    // The ground: its height, a cell's side, how far it reaches from its centre, its opacity.
    ground: vec4f,
    // The ground's centre in x and z.
    ground_centre: vec4f,
    // The colour of the ground's lines, as displayed.
    ground_ink: vec4f,
    // x: 0 shaded, 1 clay under a wireframe, 2 normals. y: 1 when the target is not sRGB, so
    // the shader encodes its colours itself.
    modes: vec4u,
}

@group(0) @binding(0) var<uniform> globals: Globals;

// A displayed colour as the target takes it.
fn to_target(displayed: vec3f) -> vec3f {
    if globals.modes.y == 1u {
        return displayed;
    }
    return pow(displayed, vec3f(2.2));
}

// A linear colour as it is displayed: sRGB's curve.
fn to_displayed(linear: vec3f) -> vec3f {
    let low = linear * 12.92;
    let high = 1.055 * pow(max(linear, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3f(0.0031308));
}
