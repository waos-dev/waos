//! The box a model fits in, which the camera frames and the ground sits under.

use glam::{Mat4, Vec3};

use super::{Model, Primitive, Skin};

/// A box along the axes.
#[derive(Clone, Copy)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    /// A box around nothing, which any point widens.
    const EMPTY: Bounds = Bounds {
        min: Vec3::INFINITY,
        max: Vec3::NEG_INFINITY,
    };

    /// The box of a model with nothing in it: a unit cube at the origin.
    const UNIT: Bounds = Bounds {
        min: Vec3::splat(-0.5),
        max: Vec3::splat(0.5),
    };

    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// The radius of the sphere around the box.
    pub fn radius(&self) -> f32 {
        ((self.max - self.min) * 0.5).length()
    }

    fn include(&mut self, point: Vec3) {
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }

    fn corners(&self) -> [Vec3; 8] {
        let pick = |corner: usize, axis: usize| {
            if corner >> axis & 1 == 0 {
                self.min[axis]
            } else {
                self.max[axis]
            }
        };
        std::array::from_fn(|corner| Vec3::new(pick(corner, 0), pick(corner, 1), pick(corner, 2)))
    }
}

/// The box around the model's meshes with its nodes at `world`: a mesh where its node puts it,
/// and a skinned one where its joints put each of its vertices.
pub fn of_model(model: &Model, world: &[Mat4]) -> Bounds {
    let mut bounds = Bounds::EMPTY;
    for scene_node in model.scene_nodes() {
        let node = &model.nodes[scene_node.index];
        let Some(mesh) = node.mesh.and_then(|mesh| model.meshes.get(mesh)) else {
            continue;
        };
        let skin = node.skin.and_then(|skin| model.skins.get(skin));
        for primitive in &mesh.primitives {
            match skin {
                Some(skin) => bounds.include_skinned(primitive, skin, world),
                None => bounds.include_placed(primitive, world[scene_node.index]),
            }
        }
    }
    if bounds.min.cmple(bounds.max).all() && bounds.radius() > 0.0 {
        bounds
    } else {
        Bounds::UNIT
    }
}

impl Bounds {
    /// Widens the box by `primitive` as `place` puts it: by the corners of its own box.
    fn include_placed(&mut self, primitive: &Primitive, place: Mat4) {
        if primitive.vertices.is_empty() {
            return;
        }
        for corner in of_primitive(primitive).corners() {
            self.include(place.transform_point3(corner));
        }
    }

    /// Widens the box by each vertex of `primitive` where the joints of `skin` put it, as the
    /// vertex shader places it.
    fn include_skinned(&mut self, primitive: &Primitive, skin: &Skin, world: &[Mat4]) {
        let joints: Vec<Mat4> = skin
            .joints
            .iter()
            .zip(&skin.inverse_bind)
            .map(|(joint, inverse_bind)| world[*joint] * *inverse_bind)
            .collect();
        for vertex in &primitive.vertices {
            let followed = vertex.joints.iter().zip(vertex.weights);
            let place = followed
                .filter_map(|(joint, weight)| Some(*joints.get(usize::from(*joint))? * weight))
                .fold(Mat4::ZERO, |sum, part| sum + part);
            self.include(place.transform_point3(Vec3::from_array(vertex.position)));
        }
    }
}

/// The box around a primitive's vertices, in the mesh's own space.
pub fn of_primitive(primitive: &Primitive) -> Bounds {
    let mut bounds = Bounds::EMPTY;
    for vertex in &primitive.vertices {
        bounds.include(Vec3::from_array(vertex.position));
    }
    bounds
}
