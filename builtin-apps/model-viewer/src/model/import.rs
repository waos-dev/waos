//! A glTF or GLB file as a `Model`, read with the `gltf` crate, which also decodes its images.
//!
//! What a file has beyond the model's shape is left out, and only that: lines and points, morph
//! targets, cameras, lights, second sets of texture coordinates and every extension but unlit
//! materials and emissive strength. glTF's indices are kept, so node 3 of the file is node 3 of the model.

use std::path::Path;

use glam::{Mat4, Quat, Vec3};
use gltf::animation::util::ReadOutputs;
use gltf::image::Format;

use super::animation::{Animation, Channel, Interpolation, Keys, Track};
use super::{
    Alpha, Filter, Image, Material, Mesh, Model, Node, Primitive, Sampler, Skin, Texture,
    Transform, Vertex, Wrap,
};

/// Reads the model in the file at `path`, a path inside the program; or says, for its user, why
/// it cannot be shown.
pub fn load(path: &str) -> Result<Model, String> {
    let name = name_of(path);
    let bytes =
        std::fs::read(path).map_err(|error| format!("{name} could not be read: {error}"))?;
    refuse_unread_extensions(&name, &bytes)?;
    let (document, buffers, images) =
        import(path, &bytes).map_err(|error| explain(&name, &error))?;
    let file = File {
        document: &document,
        buffers: &buffers,
    };
    Ok(Model {
        name,
        nodes: document.nodes().map(node).collect(),
        roots: file.roots(),
        meshes: document.meshes().map(|mesh| file.mesh(mesh)).collect(),
        materials: document.materials().map(material).collect(),
        textures: document.textures().map(texture).collect(),
        images: images.into_iter().map(rgba_image).collect(),
        skins: document.skins().map(|skin| file.skin(skin)).collect(),
        animations: document
            .animations()
            .map(|animation| file.animation(animation))
            .collect(),
    })
}

/// The file's name without its folders and its extension.
fn name_of(path: &str) -> String {
    let stem = Path::new(path).file_stem().and_then(|stem| stem.to_str());
    stem.unwrap_or("Model").to_owned()
}

/// The extensions a file may require and still be shown: the ones the app draws, and a file's
/// lights, which it passes over.
const EXTENSIONS_READ: [&str; 3] = [
    "KHR_lights_punctual",
    "KHR_materials_emissive_strength",
    "KHR_materials_unlit",
];

/// Refuses a file that requires an extension the app does not read, naming it: the `gltf` crate
/// would refuse it too, without saying which.
fn refuse_unread_extensions(name: &str, bytes: &[u8]) -> Result<(), String> {
    let Ok(file) = gltf::Gltf::from_slice_without_validation(bytes) else {
        return Ok(());
    };
    let unread = file
        .extensions_required()
        .find(|extension| !EXTENSIONS_READ.contains(extension));
    match unread {
        Some(extension) => Err(format!(
            "{name} needs {}, which the app does not read.",
            plain_name(extension)
        )),
        None => Ok(()),
    }
}

/// What an extension is, for a reader who does not know glTF's names.
fn plain_name(extension: &str) -> &str {
    match extension {
        "KHR_draco_mesh_compression" => "Draco mesh compression",
        "EXT_meshopt_compression" => "meshopt compression",
        "KHR_texture_basisu" => "KTX2 textures",
        "EXT_texture_webp" => "WebP textures",
        "KHR_mesh_quantization" => "quantized meshes",
        "KHR_texture_transform" => "texture transforms",
        other => other,
    }
}

/// A file as the `gltf` crate reads it: what it says, its buffers' bytes and its images' pixels.
type Imported = (
    gltf::Document,
    Vec<gltf::buffer::Data>,
    Vec<gltf::image::Data>,
);

/// The file's contents, read by the `gltf` crate. A binary file holds everything in `bytes`; a
/// `.gltf` may name files beside it, which are looked for from `path`.
fn import(path: &str, bytes: &[u8]) -> gltf::Result<Imported> {
    if bytes.starts_with(b"glTF") {
        gltf::import_slice(bytes)
    } else {
        gltf::import(path)
    }
}

/// Why a file did not load, for its user. A `.gltf` that keeps its buffers and images in files
/// beside it cannot be read: a program is given the file its user picked, not its folder.
fn explain(name: &str, error: &gltf::Error) -> String {
    match error {
        gltf::Error::Io(_) => format!(
            "{name} keeps its data in other files beside it, which the app is not given. Export it as a single .glb file."
        ),
        other => format!("{name} could not be read: {other}"),
    }
}

/// A parsed file with the bytes its accessors read from.
struct File<'a> {
    document: &'a gltf::Document,
    buffers: &'a [gltf::buffer::Data],
}

impl File<'_> {
    fn bytes(&self, buffer: gltf::Buffer<'_>) -> Option<&[u8]> {
        self.buffers.get(buffer.index()).map(|data| &data.0[..])
    }

    /// The roots of the scene the file opens with, or of its first; with no scene at all, every
    /// node that is no other's child.
    fn roots(&self) -> Vec<usize> {
        let scene = self
            .document
            .default_scene()
            .or_else(|| self.document.scenes().next());
        if let Some(scene) = scene {
            return scene.nodes().map(|node| node.index()).collect();
        }
        let mut is_child = vec![false; self.document.nodes().len()];
        for child in self.document.nodes().flat_map(|node| node.children()) {
            is_child[child.index()] = true;
        }
        (0..is_child.len())
            .filter(|index| !is_child[*index])
            .collect()
    }

    fn mesh(&self, mesh: gltf::Mesh<'_>) -> Mesh {
        let primitives = mesh
            .primitives()
            .filter_map(|primitive| self.primitive(primitive));
        Mesh {
            primitives: primitives.collect(),
        }
    }

    /// A primitive's triangles; none for lines and points, or for one with no positions.
    fn primitive(&self, primitive: gltf::Primitive<'_>) -> Option<Primitive> {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            return None;
        }
        let reader = primitive.reader(|buffer| self.bytes(buffer));
        let mut vertices: Vec<Vertex> = reader
            .read_positions()?
            .map(|position| Vertex::new(position.into(), Vec3::ZERO, [0.0; 2]))
            .collect();
        let normals = reader.read_normals();
        let has_normals = normals.is_some();
        fill(&mut vertices, normals, |vertex, normal| {
            vertex.normal = normal
        });
        fill(&mut vertices, reader.read_tangents(), |vertex, tangent| {
            vertex.tangent = tangent
        });
        let uvs = reader.read_tex_coords(0).map(|uvs| uvs.into_f32());
        fill(&mut vertices, uvs, |vertex, uv| vertex.uv = uv);
        let colours = reader.read_colors(0).map(|colours| colours.into_rgba_f32());
        fill(&mut vertices, colours, |vertex, colour| {
            vertex.colour = colour
        });
        let joints = reader.read_joints(0).map(|joints| joints.into_u16());
        fill(&mut vertices, joints, |vertex, joints| {
            vertex.joints = joints
        });
        let weights = reader.read_weights(0).map(|weights| weights.into_f32());
        fill(&mut vertices, weights, |vertex, weights| {
            vertex.weights = weights
        });
        let indices = match reader.read_indices() {
            Some(indices) => whole_triangles(indices.into_u32().collect(), vertices.len()),
            None => whole_triangles((0..vertices.len() as u32).collect(), vertices.len()),
        };
        if !has_normals {
            smooth_normals(&mut vertices, &indices);
        }
        Some(Primitive {
            vertices,
            indices,
            material: primitive.material().index(),
        })
    }

    fn skin(&self, skin: gltf::Skin<'_>) -> Skin {
        let joints: Vec<usize> = skin.joints().map(|joint| joint.index()).collect();
        let reader = skin.reader(|buffer| self.bytes(buffer));
        let given = reader.read_inverse_bind_matrices();
        let mut inverse_bind: Vec<Mat4> = given
            .map(|matrices| {
                matrices
                    .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                    .collect()
            })
            .unwrap_or_default();
        inverse_bind.resize(joints.len(), Mat4::IDENTITY);
        Skin {
            joints,
            inverse_bind,
        }
    }

    fn animation(&self, animation: gltf::Animation<'_>) -> Animation {
        let name = animation.name().map(str::to_owned);
        Animation {
            name: name.unwrap_or_else(|| format!("Animation {}", animation.index() + 1)),
            channels: animation
                .channels()
                .filter_map(|channel| self.channel(channel))
                .collect(),
        }
    }

    /// A channel that moves, turns or scales a node; none for one that sets morph weights.
    fn channel(&self, channel: gltf::animation::Channel<'_>) -> Option<Channel> {
        let reader = channel.reader(|buffer| self.bytes(buffer));
        let times: Vec<f32> = reader.read_inputs()?.collect();
        let interpolation = match channel.sampler().interpolation() {
            gltf::animation::Interpolation::Step => Interpolation::Step,
            gltf::animation::Interpolation::Linear => Interpolation::Linear,
            gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
        };
        let track = match reader.read_outputs()? {
            ReadOutputs::Translations(values) => Track::Translation(Keys {
                times,
                values: values.map(Vec3::from).collect(),
                interpolation,
            }),
            ReadOutputs::Rotations(values) => Track::Rotation(Keys {
                times,
                values: values.into_f32().map(Quat::from_array).collect(),
                interpolation,
            }),
            ReadOutputs::Scales(values) => Track::Scale(Keys {
                times,
                values: values.map(Vec3::from).collect(),
                interpolation,
            }),
            ReadOutputs::MorphTargetWeights(_) => return None,
        };
        Some(Channel {
            node: channel.target().node().index(),
            track,
        })
    }
}

/// Sets a part of each vertex from `values`, when the mesh has them.
fn fill<T>(
    vertices: &mut [Vertex],
    values: Option<impl Iterator<Item = T>>,
    set: impl Fn(&mut Vertex, T),
) {
    for (vertex, value) in vertices.iter_mut().zip(values.into_iter().flatten()) {
        set(vertex, value);
    }
}

/// `indices` without the triangles that name a vertex the mesh does not have, and without what
/// is left over of a last, incomplete one.
fn whole_triangles(indices: Vec<u32>, vertex_count: usize) -> Vec<u32> {
    let in_range = |triangle: &&[u32; 3]| {
        triangle
            .iter()
            .all(|index| (*index as usize) < vertex_count)
    };
    let (triangles, _incomplete) = indices.as_chunks::<3>();
    triangles
        .iter()
        .filter(in_range)
        .flatten()
        .copied()
        .collect()
}

/// Gives each vertex the average of its triangles' normals, weighted by their area: what a mesh
/// that came with no normals is lit by.
fn smooth_normals(vertices: &mut [Vertex], indices: &[u32]) {
    let mut sums = vec![Vec3::ZERO; vertices.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let corner = |index: usize| Vec3::from_array(vertices[triangle[index] as usize].position);
        let area_normal = (corner(1) - corner(0)).cross(corner(2) - corner(0));
        for index in triangle {
            sums[*index as usize] += area_normal;
        }
    }
    for (vertex, sum) in vertices.iter_mut().zip(sums) {
        vertex.normal = sum.normalize_or(Vec3::Y).to_array();
    }
}

fn node(node: gltf::Node<'_>) -> Node {
    let (translation, rotation, scale) = node.transform().decomposed();
    Node {
        children: node.children().map(|child| child.index()).collect(),
        transform: Transform {
            translation: translation.into(),
            rotation: Quat::from_array(rotation),
            scale: scale.into(),
        },
        mesh: node.mesh().map(|mesh| mesh.index()),
        skin: node.skin().map(|skin| skin.index()),
    }
}

fn material(material: gltf::Material<'_>) -> Material {
    let pbr = material.pbr_metallic_roughness();
    let texture_index =
        |info: Option<gltf::texture::Info<'_>>| info.map(|info| info.texture().index());
    let normal = material.normal_texture();
    let occlusion = material.occlusion_texture();
    let strength = material.emissive_strength().unwrap_or(1.0);
    Material {
        base_colour: pbr.base_color_factor(),
        base_colour_texture: texture_index(pbr.base_color_texture()),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        metallic_roughness_texture: texture_index(pbr.metallic_roughness_texture()),
        normal_scale: normal.as_ref().map_or(1.0, |normal| normal.scale()),
        normal_texture: normal.map(|normal| normal.texture().index()),
        occlusion_strength: occlusion
            .as_ref()
            .map_or(1.0, |occlusion| occlusion.strength()),
        occlusion_texture: occlusion.map(|occlusion| occlusion.texture().index()),
        emissive: material.emissive_factor().map(|channel| channel * strength),
        emissive_texture: texture_index(material.emissive_texture()),
        unlit: material.unlit(),
        alpha: match material.alpha_mode() {
            gltf::material::AlphaMode::Opaque => Alpha::Opaque,
            gltf::material::AlphaMode::Mask => Alpha::Mask(material.alpha_cutoff().unwrap_or(0.5)),
            gltf::material::AlphaMode::Blend => Alpha::Blend,
        },
        double_sided: material.double_sided(),
    }
}

fn texture(texture: gltf::Texture<'_>) -> Texture {
    use gltf::texture::{MagFilter, MinFilter};
    let sampler = texture.sampler();
    let (minify, mipmaps) = match sampler.min_filter() {
        Some(MinFilter::Nearest) => (Filter::Nearest, None),
        Some(MinFilter::Linear) => (Filter::Linear, None),
        Some(MinFilter::NearestMipmapNearest) => (Filter::Nearest, Some(Filter::Nearest)),
        Some(MinFilter::LinearMipmapNearest) => (Filter::Linear, Some(Filter::Nearest)),
        Some(MinFilter::NearestMipmapLinear) => (Filter::Nearest, Some(Filter::Linear)),
        Some(MinFilter::LinearMipmapLinear) | None => (Filter::Linear, Some(Filter::Linear)),
    };
    Texture {
        image: texture.source().index(),
        sampler: Sampler {
            wrap_u: wrap(sampler.wrap_s()),
            wrap_v: wrap(sampler.wrap_t()),
            magnify: match sampler.mag_filter() {
                Some(MagFilter::Nearest) => Filter::Nearest,
                Some(MagFilter::Linear) | None => Filter::Linear,
            },
            minify,
            mipmaps,
        },
    }
}

fn wrap(mode: gltf::texture::WrappingMode) -> Wrap {
    match mode {
        gltf::texture::WrappingMode::Repeat => Wrap::Repeat,
        gltf::texture::WrappingMode::MirroredRepeat => Wrap::Mirror,
        gltf::texture::WrappingMode::ClampToEdge => Wrap::Clamp,
    }
}

/// A decoded image as four bytes a pixel, whatever it was decoded to: one channel is grey, two
/// grey and alpha, three colour; sixteen bits and floats are cut to eight bits.
fn rgba_image(image: gltf::image::Data) -> Image {
    let eight_bits: Sample = (1, first_byte);
    let sixteen_bits: Sample = (2, high_byte);
    let floats: Sample = (4, float_byte);
    let (channels, (sample_size, sample)) = match image.format {
        Format::R8 => (1, eight_bits),
        Format::R8G8 => (2, eight_bits),
        Format::R8G8B8 => (3, eight_bits),
        Format::R8G8B8A8 => (4, eight_bits),
        Format::R16 => (1, sixteen_bits),
        Format::R16G16 => (2, sixteen_bits),
        Format::R16G16B16 => (3, sixteen_bits),
        Format::R16G16B16A16 => (4, sixteen_bits),
        Format::R32G32B32FLOAT => (3, floats),
        Format::R32G32B32A32FLOAT => (4, floats),
    };
    let mut pixels: Vec<u8> = image
        .pixels
        .chunks_exact(sample_size * channels)
        .flat_map(|pixel| {
            let channel = |index: usize| sample(&pixel[index * sample_size..]);
            match channels {
                1 => [channel(0), channel(0), channel(0), 255],
                2 => [channel(0), channel(0), channel(0), channel(1)],
                3 => [channel(0), channel(1), channel(2), 255],
                _ => [channel(0), channel(1), channel(2), channel(3)],
            }
        })
        .collect();
    // An image decoded short of its size is filled out, so its texture is written whole.
    pixels.resize(image.width as usize * image.height as usize * 4, 255);
    Image {
        width: image.width,
        height: image.height,
        pixels,
    }
}

/// How one channel of a decoded pixel is kept: its size in bytes, and its value cut to a byte.
type Sample = (usize, fn(&[u8]) -> u8);

fn first_byte(bytes: &[u8]) -> u8 {
    bytes[0]
}

/// A sixteen-bit sample, as the platform's bytes hold it, cut to its high eight bits.
fn high_byte(bytes: &[u8]) -> u8 {
    (u16::from_ne_bytes([bytes[0], bytes[1]]) >> 8) as u8
}

fn float_byte(bytes: &[u8]) -> u8 {
    let value = f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    (value.clamp(0.0, 1.0) * 255.0) as u8
}
