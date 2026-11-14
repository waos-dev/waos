//! The scene's pipelines and the layouts of what they read, made once for a target's format.
//!
//! Every pipeline draws into the canvas's multisampled colour and depth. A wireframe's lines are
//! a pipeline of their own over a list of edges, since WebGPU has no polygon mode that draws a
//! triangle's outline; the surfaces are pushed back a little in depth so the lines over them win.

use std::mem::size_of;

use super::canvas::{DEPTH_FORMAT, SAMPLES};
use crate::model::Vertex;

/// The layouts of the three groups a surface is drawn with.
pub struct Layouts {
    /// The frame's globals: one uniform buffer.
    pub globals: wgpu::BindGroupLayout,
    /// A draw's place and the joints of every skin.
    pub draw: wgpu::BindGroupLayout,
    /// A material's factors and its five textures, each with its sampler.
    pub material: wgpu::BindGroupLayout,
}

pub struct Pipelines {
    surfaces: [wgpu::RenderPipeline; 4],
    pub lines: wgpu::RenderPipeline,
    pub ground: wgpu::RenderPipeline,
    pub backdrop: wgpu::RenderPipeline,
}

/// How a material's surfaces are drawn, beyond what its shader reads.
#[derive(Clone, Copy, PartialEq)]
pub struct SurfaceKind {
    /// Both faces are drawn, where otherwise the back ones are culled.
    pub double_sided: bool,
    /// Blended over what is behind, without hiding it from what is drawn later.
    pub blended: bool,
}

impl SurfaceKind {
    const ALL: [SurfaceKind; 4] = [
        SurfaceKind::new(false, false),
        SurfaceKind::new(true, false),
        SurfaceKind::new(false, true),
        SurfaceKind::new(true, true),
    ];

    const fn new(double_sided: bool, blended: bool) -> SurfaceKind {
        SurfaceKind {
            double_sided,
            blended,
        }
    }

    fn index(self) -> usize {
        usize::from(self.double_sided) + 2 * usize::from(self.blended)
    }
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, layouts: &Layouts) -> Pipelines {
        let builder = Builder {
            device,
            format,
            layouts,
        };
        let model = builder.shader("model", include_str!("shaders/model.wgsl"));
        Pipelines {
            surfaces: SurfaceKind::ALL.map(|kind| builder.surfaces(&model, kind)),
            lines: builder.lines(&model),
            ground: builder.ground(),
            backdrop: builder.backdrop(),
        }
    }

    pub fn surfaces(&self, kind: SurfaceKind) -> &wgpu::RenderPipeline {
        &self.surfaces[kind.index()]
    }
}

impl Layouts {
    pub fn new(device: &wgpu::Device) -> Layouts {
        Layouts {
            globals: globals_layout(device),
            draw: draw_layout(device),
            material: material_layout(device),
        }
    }
}

fn globals_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false)],
    })
}

fn draw_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let joints = wgpu::BindGroupLayoutEntry {
        binding: 1,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("draw"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX, true), joints],
    })
}

fn material_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let mut entries = vec![uniform_entry(0, wgpu::ShaderStages::FRAGMENT, false)];
    for texture in 0..5 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1 + texture * 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 + texture * 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("material"),
        entries: &entries,
    })
}

fn uniform_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    has_dynamic_offset: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset,
            min_binding_size: None,
        },
        count: None,
    }
}

/// What every pipeline is made from.
struct Builder<'a> {
    device: &'a wgpu::Device,
    format: wgpu::TextureFormat,
    layouts: &'a Layouts,
}

/// What tells one pipeline from another.
struct Recipe<'a> {
    label: &'a str,
    layout: wgpu::PipelineLayout,
    shader: &'a wgpu::ShaderModule,
    fragment: &'a str,
    vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    blend: Option<wgpu::BlendState>,
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
    depth_bias: wgpu::DepthBiasState,
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x4,
    3 => Float32x2,
    4 => Float32x4,
    5 => Uint16x4,
    6 => Float32x4,
];

const MESH_VERTICES: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &VERTEX_ATTRIBUTES,
};

/// Surfaces sit this much deeper than they are, so a wireframe's lines over them are not lost
/// to the depth test.
const BEHIND_LINES: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 2,
    slope_scale: 1.5,
    clamp: 0.0,
};

impl Builder<'_> {
    /// A shader module of `source`, with the globals every shader of the scene reads before it.
    fn shader(&self, label: &str, source: &str) -> wgpu::ShaderModule {
        let source = format!("{}\n{source}", include_str!("shaders/globals.wgsl"));
        self.device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
    }

    fn layout(&self, groups: &[&wgpu::BindGroupLayout]) -> wgpu::PipelineLayout {
        let groups: Vec<_> = groups.iter().copied().map(Some).collect();
        self.device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &groups,
                immediate_size: 0,
            })
    }

    fn surfaces(&self, shader: &wgpu::ShaderModule, kind: SurfaceKind) -> wgpu::RenderPipeline {
        let layouts = self.layouts;
        self.pipeline(Recipe {
            label: "surfaces",
            layout: self.layout(&[&layouts.globals, &layouts.draw, &layouts.material]),
            shader,
            fragment: "fragment",
            vertex_buffers: &[Some(MESH_VERTICES)],
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: (!kind.double_sided).then_some(wgpu::Face::Back),
            blend: kind.blended.then_some(wgpu::BlendState::ALPHA_BLENDING),
            depth_write: !kind.blended,
            depth_compare: wgpu::CompareFunction::Less,
            depth_bias: BEHIND_LINES,
        })
    }

    fn lines(&self, shader: &wgpu::ShaderModule) -> wgpu::RenderPipeline {
        self.pipeline(Recipe {
            label: "lines",
            layout: self.layout(&[&self.layouts.globals, &self.layouts.draw]),
            shader,
            fragment: "line_fragment",
            vertex_buffers: &[Some(MESH_VERTICES)],
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            blend: None,
            depth_write: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            depth_bias: wgpu::DepthBiasState::default(),
        })
    }

    fn ground(&self) -> wgpu::RenderPipeline {
        self.pipeline(Recipe {
            label: "ground",
            layout: self.layout(&[&self.layouts.globals]),
            shader: &self.shader("ground", include_str!("shaders/ground.wgsl")),
            fragment: "fragment",
            vertex_buffers: &[],
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            cull_mode: None,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            depth_write: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            depth_bias: wgpu::DepthBiasState::default(),
        })
    }

    fn backdrop(&self) -> wgpu::RenderPipeline {
        self.pipeline(Recipe {
            label: "backdrop",
            layout: self.layout(&[&self.layouts.globals]),
            shader: &self.shader("backdrop", include_str!("shaders/backdrop.wgsl")),
            fragment: "fragment",
            vertex_buffers: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            blend: None,
            depth_write: false,
            depth_compare: wgpu::CompareFunction::Always,
            depth_bias: wgpu::DepthBiasState::default(),
        })
    }

    fn pipeline(&self, recipe: Recipe<'_>) -> wgpu::RenderPipeline {
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(recipe.label),
                layout: Some(&recipe.layout),
                vertex: wgpu::VertexState {
                    module: recipe.shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: recipe.vertex_buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: recipe.topology,
                    cull_mode: recipe.cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(recipe.depth_write),
                    depth_compare: Some(recipe.depth_compare),
                    stencil: Default::default(),
                    bias: recipe.depth_bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: recipe.shader,
                    entry_point: Some(recipe.fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.format,
                        blend: recipe.blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
    }
}
