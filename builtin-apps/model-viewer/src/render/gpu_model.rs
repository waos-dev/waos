//! A model on the GPU: its meshes' buffers, its textures and materials, and its list of draws,
//! one for each primitive of each node of the scene.
//!
//! The invariant: draw `n` reads its place from the `n`th slot of the draw buffer, and a skinned
//! one its joints from where `joint_starts` says its skin's begin, so `pose` writes both in the
//! order `new` laid them out.

use std::collections::HashMap;
use std::mem::size_of;

use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::mipmaps;
use super::pipelines::{Layouts, SurfaceKind};
use crate::gpu::Gpu;
use crate::model::{self, Alpha, Filter, Model, Wrap, bounds};

/// The size of a draw's slot in the draw buffer: what WebGPU aligns a uniform's offset to.
const DRAW_SLOT: usize = 256;

pub struct GpuModel {
    primitives: Vec<GpuPrimitive>,
    materials: Vec<GpuMaterial>,
    draws: Vec<Draw>,
    draw_buffer: wgpu::Buffer,
    joint_buffer: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    /// Where each skin's joints begin in the joint buffer, in matrices.
    joint_starts: Vec<u32>,
}

struct GpuPrimitive {
    vertices: wgpu::Buffer,
    triangles: wgpu::Buffer,
    triangle_index_count: u32,
    /// The triangles' edges, each once, as pairs of indices: what a wireframe draws.
    edges: wgpu::Buffer,
    edge_index_count: u32,
    material: usize,
    centre: Vec3,
}

struct GpuMaterial {
    group: wgpu::BindGroup,
    kind: SurfaceKind,
}

/// One primitive of one node.
struct Draw {
    node: usize,
    primitive: usize,
    skin: Option<usize>,
}

/// What a draw's slot holds, laid out as `Draw` in `shaders/model.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    skin: [u32; 4],
}

/// A material's factors, laid out as `Material` in `shaders/model.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    base_colour: [f32; 4],
    emissive: [f32; 4],
    factors: [f32; 4],
    cutoff: [f32; 4],
}

/// A draw as a render pass makes it.
pub struct DrawCall<'a> {
    /// Where the draw's slot is in the draw buffer: the draw group's dynamic offset.
    pub offset: u32,
    pub kind: SurfaceKind,
    pub material: &'a wgpu::BindGroup,
    pub vertices: &'a wgpu::Buffer,
    pub triangles: &'a wgpu::Buffer,
    pub triangle_index_count: u32,
    pub edges: &'a wgpu::Buffer,
    pub edge_index_count: u32,
    /// The middle of what is drawn, in the model's space.
    pub centre: Vec3,
}

impl GpuModel {
    /// Uploads `model`, with every draw placed by `world`, the matrices of its nodes.
    pub fn new(gpu: &Gpu, layouts: &Layouts, model: &Model, world: &[Mat4]) -> GpuModel {
        let primitive_starts = primitive_starts(model);
        let draws = draws(model, &primitive_starts);
        let joint_starts = joint_starts(model);
        let joint_count: usize = model.skins.iter().map(|skin| skin.joints.len()).sum();
        let draw_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draws"),
            size: (draws.len().max(1) * DRAW_SLOT) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let joint_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("joints"),
            size: (joint_count.max(1) * size_of::<Mat4>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gpu_model = GpuModel {
            primitives: primitives(&gpu.device, model),
            materials: Textures::new(gpu, model).materials(layouts),
            draw_group: draw_group(&gpu.device, layouts, &draw_buffer, &joint_buffer),
            draws,
            draw_buffer,
            joint_buffer,
            joint_starts,
        };
        gpu_model.pose(&gpu.queue, model, world);
        gpu_model
    }

    /// Places every draw, and every skin's joints, by `world`, the matrices of the nodes.
    pub fn pose(&self, queue: &wgpu::Queue, model: &Model, world: &[Mat4]) {
        let mut slots = vec![[0u8; DRAW_SLOT]; self.draws.len()];
        for (draw, slot) in self.draws.iter().zip(&mut slots) {
            let place = world[draw.node];
            let joints_start = draw.skin.map_or(0, |skin| self.joint_starts[skin]);
            let uniform = DrawUniform {
                model: place.to_cols_array_2d(),
                normal: place.inverse().transpose().to_cols_array_2d(),
                skin: [joints_start, u32::from(draw.skin.is_some()), 0, 0],
            };
            slot[..size_of::<DrawUniform>()].copy_from_slice(bytemuck::bytes_of(&uniform));
        }
        queue.write_buffer(&self.draw_buffer, 0, slots.as_flattened());
        let joints: Vec<Mat4> = model
            .skins
            .iter()
            .flat_map(|skin| skin.joints.iter().zip(&skin.inverse_bind))
            .map(|(joint, inverse_bind)| world[*joint] * *inverse_bind)
            .collect();
        queue.write_buffer(&self.joint_buffer, 0, bytemuck::cast_slice(&joints));
    }

    pub fn draw_group(&self) -> &wgpu::BindGroup {
        &self.draw_group
    }

    /// Every draw, in the order of the scene's nodes.
    pub fn draw_calls(&self) -> impl Iterator<Item = DrawCall<'_>> {
        self.draws.iter().enumerate().map(|(index, draw)| {
            let primitive = &self.primitives[draw.primitive];
            let material = &self.materials[primitive.material];
            DrawCall {
                offset: (index * DRAW_SLOT) as u32,
                kind: material.kind,
                material: &material.group,
                vertices: &primitive.vertices,
                triangles: &primitive.triangles,
                triangle_index_count: primitive.triangle_index_count,
                edges: &primitive.edges,
                edge_index_count: primitive.edge_index_count,
                centre: primitive.centre,
            }
        })
    }

    /// The node each draw belongs to, in the order of `draw_calls`.
    pub fn draw_nodes(&self) -> impl Iterator<Item = usize> {
        self.draws.iter().map(|draw| draw.node)
    }
}

/// Where each mesh's primitives begin among all the primitives, mesh after mesh.
fn primitive_starts(model: &Model) -> Vec<usize> {
    let mut next = 0;
    let start_of = |mesh: &model::Mesh| {
        let start = next;
        next += mesh.primitives.len();
        start
    };
    model.meshes.iter().map(start_of).collect()
}

fn draws(model: &Model, primitive_starts: &[usize]) -> Vec<Draw> {
    let mut draws = Vec::new();
    for scene_node in model.scene_nodes() {
        let node = &model.nodes[scene_node.index];
        let Some(mesh) = node.mesh.filter(|mesh| *mesh < model.meshes.len()) else {
            continue;
        };
        let skin = node.skin.filter(|skin| *skin < model.skins.len());
        let count = model.meshes[mesh].primitives.len();
        draws.extend((0..count).map(|primitive| Draw {
            node: scene_node.index,
            primitive: primitive_starts[mesh] + primitive,
            skin,
        }));
    }
    draws
}

fn joint_starts(model: &Model) -> Vec<u32> {
    let mut next = 0;
    let start_of = |skin: &model::Skin| {
        let start = next;
        next += skin.joints.len() as u32;
        start
    };
    model.skins.iter().map(start_of).collect()
}

fn primitives(device: &wgpu::Device, model: &Model) -> Vec<GpuPrimitive> {
    let default_material = model.materials.len();
    let all = model.meshes.iter().flat_map(|mesh| &mesh.primitives);
    all.map(|primitive| {
        let edges = edges(&primitive.indices);
        let in_range = |material: &usize| *material < default_material;
        GpuPrimitive {
            vertices: buffer(
                device,
                bytemuck::cast_slice(&primitive.vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            triangles: buffer(
                device,
                bytemuck::cast_slice(&primitive.indices),
                wgpu::BufferUsages::INDEX,
            ),
            triangle_index_count: primitive.indices.len() as u32,
            edges: buffer(
                device,
                bytemuck::cast_slice(&edges),
                wgpu::BufferUsages::INDEX,
            ),
            edge_index_count: edges.len() as u32,
            material: primitive
                .material
                .filter(in_range)
                .unwrap_or(default_material),
            centre: bounds::of_primitive(primitive).centre(),
        }
    })
    .collect()
}

fn buffer(device: &wgpu::Device, contents: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents,
        usage,
    })
}

/// The edges of `triangles`, each once however many triangles share it, as pairs of indices.
fn edges(triangles: &[u32]) -> Vec<u32> {
    let (triangles, _incomplete) = triangles.as_chunks::<3>();
    let mut edges: Vec<[u32; 2]> = triangles
        .iter()
        .flat_map(|[first, second, third]| [[*first, *second], [*second, *third], [*third, *first]])
        .map(|[from, to]| [from.min(to), from.max(to)])
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges.into_iter().flatten().collect()
}

fn draw_group(
    device: &wgpu::Device,
    layouts: &Layouts,
    draws: &wgpu::Buffer,
    joints: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let slot = wgpu::BufferBinding {
        buffer: draws,
        offset: 0,
        size: wgpu::BufferSize::new(size_of::<DrawUniform>() as u64),
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("draw"),
        layout: &layouts.draw,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(slot),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: joints.as_entire_binding(),
            },
        ],
    })
}

/// Whether a texture holds colours, which are stored as displayed, or plain numbers.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Encoding {
    Colour,
    Numbers,
}

/// A model's textures as they are uploaded: each of a material's as its use needs it encoded,
/// once however many materials share it, with a white pixel standing for one a material lacks.
struct Textures<'a> {
    gpu: &'a Gpu,
    model: &'a Model,
    uploaded: HashMap<(usize, Encoding), wgpu::TextureView>,
    samplers: HashMap<model::Sampler, wgpu::Sampler>,
    white: wgpu::TextureView,
    plain_sampler: wgpu::Sampler,
}

impl<'a> Textures<'a> {
    fn new(gpu: &'a Gpu, model: &'a Model) -> Textures<'a> {
        let white = mipmaps::chain(1, 1, vec![255; 4]);
        Textures {
            gpu,
            model,
            uploaded: HashMap::new(),
            samplers: HashMap::new(),
            white: upload(gpu, &white, Encoding::Numbers),
            plain_sampler: gpu
                .device
                .create_sampler(&wgpu::SamplerDescriptor::default()),
        }
    }

    /// A bind group for each of the model's materials, and last one for glTF's default material.
    fn materials(mut self, layouts: &Layouts) -> Vec<GpuMaterial> {
        let default = model::Material::default();
        let materials = self.model.materials.iter().chain([&default]);
        materials
            .map(|material| self.material(layouts, material))
            .collect()
    }

    fn material(&mut self, layouts: &Layouts, material: &model::Material) -> GpuMaterial {
        let uniform = MaterialUniform {
            base_colour: material.base_colour,
            emissive: [
                material.emissive[0],
                material.emissive[1],
                material.emissive[2],
                1.0,
            ],
            factors: [
                material.metallic,
                material.roughness,
                material.normal_scale,
                material.occlusion_strength,
            ],
            cutoff: [
                match material.alpha {
                    Alpha::Mask(cutoff) => cutoff,
                    Alpha::Opaque | Alpha::Blend => 0.0,
                },
                f32::from(u8::from(material.normal_texture.is_some())),
                f32::from(u8::from(material.unlit)),
                0.0,
            ],
        };
        let factors = buffer(
            &self.gpu.device,
            bytemuck::bytes_of(&uniform),
            wgpu::BufferUsages::UNIFORM,
        );
        let textures = [
            (material.base_colour_texture, Encoding::Colour),
            (material.metallic_roughness_texture, Encoding::Numbers),
            (material.normal_texture, Encoding::Numbers),
            (material.occlusion_texture, Encoding::Numbers),
            (material.emissive_texture, Encoding::Colour),
        ]
        .map(|(texture, encoding)| self.texture(texture, encoding));
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: factors.as_entire_binding(),
        }];
        for (index, (view, sampler)) in textures.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + index as u32 * 2,
                resource: wgpu::BindingResource::TextureView(view),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 2 + index as u32 * 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            });
        }
        let group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout: &layouts.material,
                entries: &entries,
            });
        GpuMaterial {
            group,
            kind: SurfaceKind {
                double_sided: material.double_sided,
                blended: material.alpha == Alpha::Blend,
            },
        }
    }

    /// The view and sampler of one of the model's textures, uploaded at its first use; the white
    /// pixel for none, or for one whose image the model does not have.
    fn texture(
        &mut self,
        texture: Option<usize>,
        encoding: Encoding,
    ) -> (wgpu::TextureView, wgpu::Sampler) {
        let model = self.model;
        let found = texture.and_then(|index| model.textures.get(index));
        let image = found.and_then(|texture| Some((texture, model.images.get(texture.image)?)));
        let Some((texture, image)) = image else {
            return (self.white.clone(), self.plain_sampler.clone());
        };
        let gpu = self.gpu;
        let view = self
            .uploaded
            .entry((texture.image, encoding))
            .or_insert_with(|| {
                let levels = mipmaps::chain(image.width, image.height, image.pixels.clone());
                upload(gpu, &levels, encoding)
            });
        let sampler = self
            .samplers
            .entry(texture.sampler)
            .or_insert_with(|| sampler(&gpu.device, texture.sampler));
        (view.clone(), sampler.clone())
    }
}

/// A texture of `levels`, the largest first, leaving out those too large for the device.
fn upload(gpu: &Gpu, levels: &[mipmaps::Level], encoding: Encoding) -> wgpu::TextureView {
    let limit = gpu.device.limits().max_texture_dimension_2d;
    let fits = |level: &&mipmaps::Level| level.width <= limit && level.height <= limit;
    let levels: Vec<&mipmaps::Level> = levels.iter().filter(fits).collect();
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent(levels[0]),
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: match encoding {
            Encoding::Colour => wgpu::TextureFormat::Rgba8UnormSrgb,
            Encoding::Numbers => wgpu::TextureFormat::Rgba8Unorm,
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip_level, level) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip_level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(level.width * 4),
                rows_per_image: Some(level.height),
            },
            extent(level),
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn extent(level: &mipmaps::Level) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: level.width,
        height: level.height,
        depth_or_array_layers: 1,
    }
}

fn sampler(device: &wgpu::Device, sampler: model::Sampler) -> wgpu::Sampler {
    let wrap = |wrap| match wrap {
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
    };
    let filter = |filter| match filter {
        Filter::Nearest => wgpu::FilterMode::Nearest,
        Filter::Linear => wgpu::FilterMode::Linear,
    };
    let smooth = [sampler.magnify, sampler.minify] == [Filter::Linear; 2]
        && sampler.mipmaps == Some(Filter::Linear);
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: None,
        address_mode_u: wrap(sampler.wrap_u),
        address_mode_v: wrap(sampler.wrap_v),
        mag_filter: filter(sampler.magnify),
        min_filter: filter(sampler.minify),
        mipmap_filter: match sampler.mipmaps {
            Some(Filter::Linear) => wgpu::MipmapFilterMode::Linear,
            Some(Filter::Nearest) | None => wgpu::MipmapFilterMode::Nearest,
        },
        // Without mipmaps, only the image itself is sampled.
        lod_max_clamp: if sampler.mipmaps.is_some() { 32.0 } else { 0.0 },
        // Sharper at a slant, which WebGPU allows only where every filter is linear.
        anisotropy_clamp: if smooth { 8 } else { 1 },
        ..Default::default()
    })
}
