// A model's surfaces: glTF's metal and roughness material under one light and an environment.
//
// The environment is a formula, not a picture: a dim room, paler above than at the horizon, over
// a dark floor, with two soft lights in it, all blurred by a surface's roughness. It lights every
// surface from all around (its diffuse part) and shows in what is smooth (its specular part,
// with Karis's fit of the environment's response). The light is a directional one, without
// shadows. Colours come out through Khronos's PBR Neutral tone mapper.

struct Draw {
    model: mat4x4f,
    // The matrix normals are turned by: the model's inverse transpose.
    normal: mat4x4f,
    // x: where the draw's joints begin among the joints. y: 1 for a skinned mesh.
    skin: vec4u,
}

@group(1) @binding(0) var<uniform> draw: Draw;
@group(1) @binding(1) var<storage, read> joints: array<mat4x4f>;

struct Material {
    base_colour: vec4f,
    emissive: vec4f,
    // Metal, roughness, the normal map's scale, the occlusion's strength.
    factors: vec4f,
    // x: the alpha below which a masked material is not drawn, or zero. y: 1 with a normal map.
    // z: 1 for an unlit material.
    cutoff: vec4f,
}

@group(2) @binding(0) var<uniform> material: Material;
@group(2) @binding(1) var base_colour_texture: texture_2d<f32>;
@group(2) @binding(2) var base_colour_sampler: sampler;
@group(2) @binding(3) var metallic_roughness_texture: texture_2d<f32>;
@group(2) @binding(4) var metallic_roughness_sampler: sampler;
@group(2) @binding(5) var normal_texture: texture_2d<f32>;
@group(2) @binding(6) var normal_sampler: sampler;
@group(2) @binding(7) var occlusion_texture: texture_2d<f32>;
@group(2) @binding(8) var occlusion_sampler: sampler;
@group(2) @binding(9) var emissive_texture: texture_2d<f32>;
@group(2) @binding(10) var emissive_sampler: sampler;

struct MeshVertex {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) tangent: vec4f,
    @location(3) uv: vec2f,
    @location(4) colour: vec4f,
    @location(5) joints: vec4u,
    @location(6) weights: vec4f,
}

struct SurfacePoint {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) tangent: vec4f,
    @location(3) uv: vec2f,
    @location(4) colour: vec4f,
}

// The matrix that places a vertex: the draw's own, or the blend of its joints'.
fn placement(vertex: MeshVertex) -> mat4x4f {
    if draw.skin.y == 0u {
        return draw.model;
    }
    let first = draw.skin.x;
    return vertex.weights.x * joints[first + vertex.joints.x]
        + vertex.weights.y * joints[first + vertex.joints.y]
        + vertex.weights.z * joints[first + vertex.joints.z]
        + vertex.weights.w * joints[first + vertex.joints.w];
}

@vertex
fn vertex(vertex: MeshVertex) -> SurfacePoint {
    let place = placement(vertex);
    let turn = mat3x3f(place[0].xyz, place[1].xyz, place[2].xyz);
    var normal_turn = mat3x3f(draw.normal[0].xyz, draw.normal[1].xyz, draw.normal[2].xyz);
    if draw.skin.y != 0u {
        normal_turn = turn;
    }
    let world = place * vec4f(vertex.position, 1.0);
    var point: SurfacePoint;
    point.clip = globals.view_projection * world;
    point.world = world.xyz;
    point.normal = normal_turn * vertex.normal;
    point.tangent = vec4f(turn * vertex.tangent.xyz, vertex.tangent.w);
    point.uv = vertex.uv;
    point.colour = vertex.colour;
    return point;
}

const PI: f32 = 3.14159265;
const ZENITH: vec3f = vec3f(0.5, 0.53, 0.58);
const HORIZON: vec3f = vec3f(0.34, 0.35, 0.37);
const FLOOR: vec3f = vec3f(0.07, 0.068, 0.065);
// The soft lights in the environment, as a studio has them: a warm key from the front and
// above, and a cooler one from behind. Each is where it is, with its colour.
const KEY_LIGHT: vec3f = vec3f(-0.45, 0.72, 0.53);
const KEY_COLOUR: vec3f = vec3f(3.4, 3.25, 3.0);
const BACK_LIGHT: vec3f = vec3f(0.62, 0.38, -0.69);
const BACK_COLOUR: vec3f = vec3f(1.5, 1.7, 2.0);
const LIGHT_COLOUR: vec3f = vec3f(1.0, 0.96, 0.9);

// A soft light's glow in `direction`, tight for a smooth surface and wide and dim for a rough
// one.
fn soft_light(direction: vec3f, towards: vec3f, colour: vec3f, roughness: f32) -> vec3f {
    let tightness = mix(26.0, 1.4, sqrt(roughness));
    let glow = exp2(-tightness * (1.0 - dot(direction, normalize(towards))));
    return colour * glow * mix(1.0, 0.2, roughness);
}

// The environment's light from `direction`, as a surface of `roughness` sees it: the rougher,
// the more of the surroundings it gathers, so the horizon softens and the lights spread.
fn environment(direction: vec3f, roughness: f32) -> vec3f {
    let height = direction.y / mix(0.12, 1.2, roughness);
    let sky = mix(HORIZON, ZENITH, clamp(height, 0.0, 1.0));
    let below = mix(HORIZON, FLOOR, clamp(-height * 2.0, 0.0, 1.0));
    let surroundings = select(below, sky, height >= 0.0);
    return surroundings
        + soft_light(direction, KEY_LIGHT, KEY_COLOUR, roughness)
        + soft_light(direction, BACK_LIGHT, BACK_COLOUR, roughness);
}

// Karis's fit of how much of the environment a surface reflects towards the eye.
fn environment_response(specular: vec3f, roughness: f32, facing: f32) -> vec3f {
    let fit = roughness * vec4f(-1.0, -0.0275, -0.572, 0.022) + vec4f(1.0, 0.0425, 1.04, -0.04);
    let blend = min(fit.x * fit.x, exp2(-9.28 * facing)) * fit.x + fit.y;
    let scale_and_bias = vec2f(-1.04, 1.04) * blend + fit.zw;
    return specular * scale_and_bias.x + scale_and_bias.y;
}

// What the directional light adds: Lambert's diffuse, and a specular lobe of GGX's
// distribution, Smith's height-correlated visibility and Schlick's Fresnel.
fn lit(normal: vec3f, to_eye: vec3f, diffuse: vec3f, specular: vec3f, roughness: f32) -> vec3f {
    let to_light = normalize(globals.light.xyz);
    let half_way = normalize(to_light + to_eye);
    let facing_light = max(dot(normal, to_light), 0.0);
    let facing_eye = max(dot(normal, to_eye), 1e-4);
    let facing_half = max(dot(normal, half_way), 0.0);
    let alpha = roughness * roughness;
    let alpha_squared = alpha * alpha;
    let spread = facing_half * facing_half * (alpha_squared - 1.0) + 1.0;
    let distribution = alpha_squared / (PI * spread * spread);
    let seen_from_eye = facing_light * sqrt(facing_eye * facing_eye * (1.0 - alpha_squared) + alpha_squared);
    let seen_from_light = facing_eye * sqrt(facing_light * facing_light * (1.0 - alpha_squared) + alpha_squared);
    let visibility = 0.5 / max(seen_from_eye + seen_from_light, 1e-5);
    let fresnel = specular + (1.0 - specular) * pow(1.0 - max(dot(half_way, to_eye), 0.0), 5.0);
    let reflected = (diffuse / PI) * (1.0 - fresnel) + fresnel * distribution * visibility;
    return reflected * LIGHT_COLOUR * globals.light.w * facing_light;
}

// Khronos's PBR Neutral tone mapper: colours keep their hue and saturation up to bright
// highlights, which roll off to white.
fn tone_mapped(colour_in: vec3f) -> vec3f {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    var colour = colour_in;
    let darkest = min(colour.r, min(colour.g, colour.b));
    let offset = select(0.04, darkest - 6.25 * darkest * darkest, darkest < 0.08);
    colour -= offset;
    let peak = max(colour.r, max(colour.g, colour.b));
    if peak < start {
        return colour;
    }
    let headroom = 1.0 - start;
    let new_peak = 1.0 - headroom * headroom / (peak + headroom - start);
    colour *= new_peak / peak;
    let whiten = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(colour, vec3f(new_peak), whiten);
}

// The normal a normal map's texel stands for, in the model's space: along the mesh's tangent
// when it has one, or else along the directions the texture coordinates grow in on screen.
fn mapped_normal(point: SurfacePoint, normal: vec3f, texel: vec3f, world_dx: vec3f, world_dy: vec3f, uv_dx: vec2f, uv_dy: vec2f) -> vec3f {
    var tangent = point.tangent.xyz;
    var bitangent = cross(normal, tangent) * point.tangent.w;
    if dot(tangent, tangent) < 1e-8 {
        let across = cross(world_dy, normal);
        let along = cross(normal, world_dx);
        tangent = across * uv_dx.x + along * uv_dy.x;
        bitangent = across * uv_dx.y + along * uv_dy.y;
        let longest = max(dot(tangent, tangent), dot(bitangent, bitangent));
        if longest < 1e-16 {
            return normal;
        }
        let shrink = inverseSqrt(longest);
        tangent *= shrink;
        bitangent *= shrink;
    } else {
        tangent = normalize(tangent);
        bitangent = normalize(bitangent);
    }
    let tilt = (texel * 2.0 - 1.0) * vec3f(material.factors.z, material.factors.z, 1.0);
    return normalize(tangent * tilt.x + bitangent * tilt.y + normal * tilt.z);
}

@fragment
fn fragment(point: SurfacePoint, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    // Every texture is sampled before anything branches, as derivatives require.
    let base_texel = textureSample(base_colour_texture, base_colour_sampler, point.uv);
    let metallic_roughness_texel = textureSample(metallic_roughness_texture, metallic_roughness_sampler, point.uv);
    let normal_texel = textureSample(normal_texture, normal_sampler, point.uv);
    let occlusion_texel = textureSample(occlusion_texture, occlusion_sampler, point.uv);
    let emissive_texel = textureSample(emissive_texture, emissive_sampler, point.uv);
    let world_dx = dpdx(point.world);
    let world_dy = dpdy(point.world);
    let uv_dx = dpdx(point.uv);
    let uv_dy = dpdy(point.uv);

    let base = material.base_colour * point.colour * base_texel;
    if base.a < material.cutoff.x {
        discard;
    }
    var normal = normalize(point.normal);
    if !front {
        normal = -normal;
    }
    if material.cutoff.y == 1.0 {
        normal = mapped_normal(point, normal, normal_texel.xyz, world_dx, world_dy, uv_dx, uv_dy);
    }
    if globals.modes.x == 2u {
        return vec4f(to_target(normal * 0.5 + 0.5), 1.0);
    }

    if material.cutoff.z == 1.0 && globals.modes.x == 0u {
        return vec4f(to_target(to_displayed(base.rgb)), base.a);
    }

    var colour = base.rgb;
    var metallic = clamp(material.factors.x * metallic_roughness_texel.b, 0.0, 1.0);
    var roughness = clamp(material.factors.y * metallic_roughness_texel.g, 0.04, 1.0);
    var emissive = material.emissive.rgb * emissive_texel.rgb;
    var occlusion = 1.0 + material.factors.w * (occlusion_texel.r - 1.0);
    if globals.modes.x == 1u {
        colour = vec3f(0.78);
        metallic = 0.0;
        roughness = 0.7;
        emissive = vec3f(0.0);
        occlusion = 1.0;
    }

    let to_eye = normalize(globals.camera.xyz - point.world);
    let facing = max(dot(normal, to_eye), 1e-4);
    let diffuse = colour * (1.0 - metallic);
    let specular = mix(vec3f(0.04), colour, metallic);
    let mirrored = reflect(-to_eye, normal);
    let around = diffuse * environment(normal, 1.0)
        + environment(mirrored, roughness) * environment_response(specular, roughness, facing);
    let light = around * occlusion + lit(normal, to_eye, diffuse, specular, roughness) + emissive;
    let displayed = to_displayed(tone_mapped(light * globals.camera.w));
    return vec4f(to_target(displayed), base.a);
}

// A wireframe's line: one dark ink over the clay, whatever the material.
@fragment
fn line_fragment(point: SurfacePoint) -> @location(0) vec4f {
    return vec4f(to_target(vec3f(0.13, 0.15, 0.2)), 1.0);
}
