//! A model as the app holds it, whatever it came from: a file or the built-in sample.
//!
//! The shape is glTF's, cut down to what the renderer draws: nodes in a tree, each with a
//! transform and perhaps a mesh and a skin; meshes of triangle lists; metal and roughness
//! materials with their five textures; skins; and animations of the nodes' transforms. Every
//! index (`usize`) is into the `Vec` of the same name on the model.

pub mod animation;
pub mod bounds;
pub mod import;
pub mod pose;
pub mod sample;

use glam::{Mat4, Quat, Vec3};

use self::animation::Animation;

pub struct Model {
    pub name: String,
    pub nodes: Vec<Node>,
    /// The nodes with no parent, which the scene shown is made of.
    pub roots: Vec<usize>,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub images: Vec<Image>,
    pub skins: Vec<Skin>,
    pub animations: Vec<Animation>,
}

impl Model {
    /// The nodes of the scene shown: the roots and everything under them, each once, a parent
    /// before its children. A file may link its nodes in a loop, which ends here.
    pub fn scene_nodes(&self) -> Vec<SceneNode> {
        let mut found = Vec::new();
        let mut seen = vec![false; self.nodes.len()];
        let mut pending: Vec<SceneNode> = self
            .roots
            .iter()
            .rev()
            .map(|root| SceneNode::root(*root))
            .collect();
        while let Some(node) = pending.pop() {
            if node.index >= seen.len() || std::mem::replace(&mut seen[node.index], true) {
                continue;
            }
            let children = self.nodes[node.index].children.iter().rev();
            pending.extend(children.map(|child| SceneNode {
                index: *child,
                parent: Some(node.index),
            }));
            found.push(node);
        }
        found
    }

    pub fn triangle_count(&self) -> usize {
        let primitives = self.meshes.iter().flat_map(|mesh| &mesh.primitives);
        primitives
            .map(|primitive| primitive.indices.len() / 3)
            .sum()
    }
}

/// A node of the scene shown, with the node it hangs from.
#[derive(Clone, Copy)]
pub struct SceneNode {
    pub index: usize,
    pub parent: Option<usize>,
}

impl SceneNode {
    fn root(index: usize) -> SceneNode {
        SceneNode {
            index,
            parent: None,
        }
    }
}

#[derive(Default)]
pub struct Node {
    pub children: Vec<usize>,
    pub transform: Transform,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
}

/// A node's place in its parent: scaled, then turned, then moved.
#[derive(Clone, Copy)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Transform {
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

pub struct Mesh {
    pub primitives: Vec<Primitive>,
}

/// A list of triangles in one material.
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// None for glTF's default material: white, rough, not metal.
    pub material: Option<usize>,
}

/// A vertex as the GPU reads it. A tangent of no length means the mesh gave none, and the shader
/// finds one from the texture coordinates. Joints and weights are zero on a mesh with no skin.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv: [f32; 2],
    pub colour: [f32; 4],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

impl Vertex {
    /// A vertex at `position` facing `normal`, white, with no tangent and no skin.
    pub fn new(position: Vec3, normal: Vec3, uv: [f32; 2]) -> Vertex {
        Vertex {
            position: position.to_array(),
            normal: normal.to_array(),
            tangent: [0.0; 4],
            uv,
            colour: [1.0; 4],
            joints: [0; 4],
            weights: [0.0; 4],
        }
    }
}

/// glTF's metal and roughness material. A texture multiplies the factor beside it.
pub struct Material {
    pub base_colour: [f32; 4],
    pub base_colour_texture: Option<usize>,
    pub metallic: f32,
    pub roughness: f32,
    /// Roughness in its green channel and metal in its blue.
    pub metallic_roughness_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub normal_scale: f32,
    pub occlusion_texture: Option<usize>,
    pub occlusion_strength: f32,
    pub emissive: [f32; 3],
    pub emissive_texture: Option<usize>,
    /// Drawn in its base colour alone, as no light falls on it.
    pub unlit: bool,
    pub alpha: Alpha,
    pub double_sided: bool,
}

impl Default for Material {
    fn default() -> Material {
        Material {
            base_colour: [1.0; 4],
            base_colour_texture: None,
            metallic: 1.0,
            roughness: 1.0,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            occlusion_strength: 1.0,
            emissive: [0.0; 3],
            emissive_texture: None,
            unlit: false,
            alpha: Alpha::Opaque,
            double_sided: false,
        }
    }
}

/// What a material does with its base colour's alpha.
#[derive(Clone, Copy, PartialEq)]
pub enum Alpha {
    Opaque,
    /// Drawn where alpha reaches the cutoff, and not at all elsewhere.
    Mask(f32),
    Blend,
}

/// An image with how it is sampled.
pub struct Texture {
    pub image: usize,
    pub sampler: Sampler,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sampler {
    pub wrap_u: Wrap,
    pub wrap_v: Wrap,
    pub magnify: Filter,
    pub minify: Filter,
    /// How the smaller copies of the image are blended; none to sample only the image itself.
    pub mipmaps: Option<Filter>,
}

impl Default for Sampler {
    fn default() -> Sampler {
        Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            magnify: Filter::Linear,
            minify: Filter::Linear,
            mipmaps: Some(Filter::Linear),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wrap {
    Repeat,
    Mirror,
    Clamp,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Filter {
    Nearest,
    Linear,
}

/// An image's pixels, four bytes each: red, green, blue, alpha.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The nodes a skinned mesh's vertices follow, each with the matrix that brings a vertex from
/// the mesh's space into the joint's.
pub struct Skin {
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
}
