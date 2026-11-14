//! The model the app opens with, made in code: a gilded knot between a glazed sphere and a
//! copper one, on a tiled plinth, with two animations of the knot. It shows what the renderer
//! draws (metal against glaze, a texture for each of colour, roughness and relief) through the
//! same `Model` a file becomes.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};

use super::animation::{Animation, Channel, Interpolation, Keys, Track};
use super::{Image, Material, Mesh, Model, Node, Primitive, Sampler, Texture, Transform, Vertex};

/// A shape's vertices and its triangles' indices.
type Shape = (Vec<Vertex>, Vec<u32>);

const KNOT_NODE: usize = 1;
/// The materials, as `materials` lists them.
const TILES_MATERIAL: usize = 0;
const STONE: usize = 1;
const GOLD: usize = 2;
const GLAZE: usize = 3;
const COPPER: usize = 4;
/// Where the knot rests: above the plinth's middle.
const KNOT_REST: Vec3 = Vec3::new(0.0, 0.62, 0.0);
/// The side of each of the plinth's textures, in pixels.
const TEXTURE_SIDE: u32 = 256;
/// The tiles across the plinth's textures.
const TILES: f32 = 8.0;
/// The radius of the plinth's flat top.
const PLINTH_TOP: f32 = 1.02;

pub fn model() -> Model {
    let plinth = Node {
        mesh: Some(0),
        ..Node::default()
    };
    let knot = Node {
        mesh: Some(1),
        transform: Transform {
            translation: KNOT_REST,
            ..Transform::default()
        },
        ..Node::default()
    };
    let glazed = sphere_node(2, Vec3::new(-0.62, 0.2, 0.36));
    let copper = sphere_node(3, Vec3::new(0.64, 0.16, 0.3));
    Model {
        name: "Sample".into(),
        nodes: vec![plinth, knot, glazed, copper],
        roots: vec![0, 1, 2, 3],
        meshes: vec![
            Mesh {
                primitives: vec![
                    primitive(plinth_top(), TILES_MATERIAL),
                    primitive(plinth_rim(), STONE),
                ],
            },
            mesh(knot_shape(), GOLD),
            mesh(sphere_shape(0.2), GLAZE),
            mesh(sphere_shape(0.16), COPPER),
        ],
        materials: materials(),
        textures: (0..3).map(tile_texture).collect(),
        images: vec![tile_colours(), tile_roughness(), tile_relief()],
        skins: Vec::new(),
        animations: vec![turn(), float()],
    }
}

fn sphere_node(mesh: usize, translation: Vec3) -> Node {
    Node {
        mesh: Some(mesh),
        transform: Transform {
            translation,
            ..Transform::default()
        },
        ..Node::default()
    }
}

fn mesh(shape: Shape, material: usize) -> Mesh {
    Mesh {
        primitives: vec![primitive(shape, material)],
    }
}

fn primitive((vertices, indices): Shape, material: usize) -> Primitive {
    Primitive {
        vertices,
        indices,
        material: Some(material),
    }
}

fn tile_texture(image: usize) -> Texture {
    Texture {
        image,
        sampler: Sampler::default(),
    }
}

/// The plinth's tiles and its stone rim, gold, a blue glaze and brushed copper.
fn materials() -> Vec<Material> {
    let tiles = Material {
        base_colour_texture: Some(0),
        metallic: 0.0,
        metallic_roughness_texture: Some(1),
        normal_texture: Some(2),
        ..Material::default()
    };
    let stone = Material {
        base_colour: [0.8, 0.78, 0.75, 1.0],
        metallic: 0.0,
        roughness: 0.62,
        ..Material::default()
    };
    let gold = Material {
        base_colour: [1.0, 0.77, 0.34, 1.0],
        roughness: 0.22,
        ..Material::default()
    };
    let glaze = Material {
        base_colour: [0.07, 0.22, 0.52, 1.0],
        metallic: 0.0,
        roughness: 0.12,
        ..Material::default()
    };
    let copper = Material {
        base_colour: [0.95, 0.64, 0.54, 1.0],
        roughness: 0.48,
        ..Material::default()
    };
    vec![tiles, stone, gold, glaze, copper]
}

/// A surface of revolution around the y axis: `profile` is its outline from the top down, each
/// point a radius and a height, turned through `segments` steps. Normals are the outline's own,
/// so a corner in it is drawn rounded.
fn lathe(profile: &[Vec2], segments: u32, uv_of: impl Fn(Vec3) -> [f32; 2]) -> Shape {
    let mut vertices = Vec::new();
    for (index, point) in profile.iter().enumerate() {
        let before = profile[index.saturating_sub(1)];
        let after = profile[(index + 1).min(profile.len() - 1)];
        let along = (after - before).normalize_or_zero();
        // The outline runs downwards on the outside, so its right-hand side faces out.
        let outward = Vec2::new(-along.y, along.x);
        for step in 0..=segments {
            let (sin, cos) = (step as f32 / segments as f32 * TAU).sin_cos();
            let position = Vec3::new(point.x * cos, point.y, point.x * sin);
            let normal = Vec3::new(outward.x * cos, outward.y, outward.x * sin);
            vertices.push(Vertex::new(position, normal, uv_of(position)));
        }
    }
    let indices = grid_indices(profile.len() as u32 - 1, segments);
    (vertices, indices)
}

/// Two triangles for each cell of a grid of `rows` by `columns` cells, whose vertices are laid
/// out row after row, each row one vertex longer than it has cells.
fn grid_indices(rows: u32, columns: u32) -> Vec<u32> {
    let mut indices = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let corner = row * (columns + 1) + column;
            let below = corner + columns + 1;
            indices.extend([corner, corner + 1, below, corner + 1, below + 1, below]);
        }
    }
    indices
}

/// The top of a low round plinth, at the ground: a disc, its tiles laid flat.
fn plinth_top() -> Shape {
    let rings = [0.0, 0.26, 0.52, 0.78, PLINTH_TOP];
    let profile = rings.map(|radius| Vec2::new(radius, 0.0));
    lathe(&profile, 72, |position| [position.x, position.z])
}

/// The plinth's bevelled rim, from its top's edge down.
fn plinth_rim() -> Shape {
    let profile = [
        Vec2::new(PLINTH_TOP, 0.0),
        Vec2::new(1.1, -0.03),
        Vec2::new(1.13, -0.09),
        Vec2::new(1.13, -0.16),
    ];
    lathe(&profile, 72, |_| [0.0, 0.0])
}

fn sphere_shape(radius: f32) -> Shape {
    const RINGS: usize = 24;
    let profile: Vec<Vec2> = (0..=RINGS)
        .map(|ring| {
            let (sin, cos) = (ring as f32 / RINGS as f32 * PI).sin_cos();
            Vec2::new(radius * sin, radius * cos)
        })
        .collect();
    lathe(&profile, 48, |_| [0.0, 0.0])
}

/// A trefoil knot: a tube around a curve that winds twice around a torus's axis while it passes
/// three times through its hole.
fn knot_shape() -> Shape {
    const ALONG: u32 = 192;
    const AROUND: u32 = 20;
    const TUBE: f32 = 0.075;
    let mut vertices = Vec::new();
    for step in 0..=ALONG {
        let turn = step as f32 / ALONG as f32 * TAU;
        let centre = knot_curve(turn);
        let forward = (knot_curve(turn + 0.001) - knot_curve(turn - 0.001)).normalize();
        // The curve never runs along the y axis for long, so y gives a steady frame around it.
        let side = forward.cross(Vec3::Y).normalize();
        let up = side.cross(forward);
        for ring in 0..=AROUND {
            // Clockwise around the curve's direction, so the triangles face out of the tube.
            let (sin, cos) = (-(ring as f32) / AROUND as f32 * TAU).sin_cos();
            let normal = side * cos + up * sin;
            vertices.push(Vertex::new(centre + normal * TUBE, normal, [0.0, 0.0]));
        }
    }
    (vertices, grid_indices(ALONG, AROUND))
}

fn knot_curve(turn: f32) -> Vec3 {
    const RADIUS: f32 = 0.24;
    let reach = RADIUS * (1.0 + 0.45 * (3.0 * turn).cos());
    Vec3::new(
        reach * (2.0 * turn).cos(),
        RADIUS * 0.6 * (3.0 * turn).sin(),
        reach * (2.0 * turn).sin(),
    )
}

/// One full turn of the knot about the y axis, at a steady rate.
fn turn() -> Animation {
    const SECONDS: f32 = 12.0;
    let quarter_turns = 0..=4;
    Animation {
        name: "Turn".into(),
        channels: vec![Channel {
            node: KNOT_NODE,
            track: Track::Rotation(Keys {
                times: quarter_turns
                    .clone()
                    .map(|step| step as f32 * SECONDS / 4.0)
                    .collect(),
                values: quarter_turns
                    .map(|step| Quat::from_rotation_y(step as f32 * TAU / 4.0))
                    .collect(),
                interpolation: Interpolation::Linear,
            }),
        }],
    }
}

/// The knot rising and settling while it tilts, eased by a cubic spline with flat tangents.
fn float() -> Animation {
    let heights = [0.0, 0.14, 0.0, 0.14, 0.0];
    let tilts = [0.0, 0.5, 0.0, -0.5, 0.0];
    let times: Vec<f32> = (0..heights.len()).map(|key| key as f32 * 1.5).collect();
    let flat = |value| [Vec3::ZERO, value, Vec3::ZERO];
    let still = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    Animation {
        name: "Float".into(),
        channels: vec![
            Channel {
                node: KNOT_NODE,
                track: Track::Translation(Keys {
                    times: times.clone(),
                    values: heights
                        .iter()
                        .flat_map(|height| flat(KNOT_REST + Vec3::Y * *height))
                        .collect(),
                    interpolation: Interpolation::CubicSpline,
                }),
            },
            Channel {
                node: KNOT_NODE,
                track: Track::Rotation(Keys {
                    times,
                    values: tilts
                        .iter()
                        .flat_map(|tilt| [still, Quat::from_rotation_x(*tilt), still])
                        .collect(),
                    interpolation: Interpolation::CubicSpline,
                }),
            },
        ],
    }
}

/// How far `coordinate` is into the gap between two tiles: 0 on a tile, rising to 1 at the
/// gap's middle.
fn gap(coordinate: f32) -> f32 {
    const HALF_WIDTH: f32 = 0.06;
    let from_edge = (coordinate.fract() - 0.5).abs();
    ((from_edge - (0.5 - HALF_WIDTH)) / HALF_WIDTH).clamp(0.0, 1.0)
}

/// An image whose pixel at tile coordinates (`u`, `v`) is `pixel(u, v)`, the pattern repeating
/// with the image.
fn tile_image(pixel: impl Fn(f32, f32) -> [u8; 4]) -> Image {
    let coordinate = |index: u32| (index as f32 + 0.5) / TEXTURE_SIDE as f32 * TILES;
    let pixels = (0..TEXTURE_SIDE)
        .flat_map(|y| (0..TEXTURE_SIDE).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(coordinate(x), coordinate(y)))
        .collect();
    Image {
        width: TEXTURE_SIDE,
        height: TEXTURE_SIDE,
        pixels,
    }
}

/// Tiles of two warm greys, with darker gaps.
fn tile_colours() -> Image {
    tile_image(|u, v| {
        let odd = (u.floor() + v.floor()) as i32 % 2 != 0;
        let tile = if odd { 196.0 } else { 222.0 };
        let shade = tile * (1.0 - 0.45 * gap(u).max(gap(v)));
        [shade as u8, (shade * 0.98) as u8, (shade * 0.95) as u8, 255]
    })
}

/// Smooth tiles and rough gaps: roughness in green, no metal in blue.
fn tile_roughness() -> Image {
    tile_image(|u, v| {
        let odd = (u.floor() + v.floor()) as i32 % 2 != 0;
        let tile = if odd { 0.34 } else { 0.22 };
        let roughness: f32 = tile + (0.9 - tile) * gap(u).max(gap(v));
        [255, (roughness * 255.0) as u8, 0, 255]
    })
}

/// A normal map of tiles whose edges slope down into the gaps.
fn tile_relief() -> Image {
    const SLOPE: f32 = 0.9;
    tile_image(|u, v| {
        let slope = |coordinate: f32| {
            let rising = gap(coordinate) > 0.0 && gap(coordinate) < 1.0;
            // A tile's surface rises away from the gap's middle, and its normal leans back to it.
            let side = if coordinate.fract() < 0.5 { -1.0 } else { 1.0 };
            if rising { side * SLOPE } else { 0.0 }
        };
        let normal = Vec3::new(slope(u), slope(v), 1.0).normalize();
        let byte = |component: f32| ((component * 0.5 + 0.5) * 255.0) as u8;
        [byte(normal.x), byte(normal.y), byte(normal.z), 255]
    })
}
