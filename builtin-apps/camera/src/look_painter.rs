//! Draws a picture through a look, on a WebGPU device of the app's own: one texture sampled, the
//! look's shader (`look.wgsl`) run over every pixel of another. Waft keeps its renderer's device
//! to itself, so the app draws on this one and shows what it drew through hardware buffers, which
//! both devices and the system's compositor reach.
//!
//! The invariant a later editor will break: `GRADE_FLOATS` and [`grade_bytes`] lay the numbers
//! out as `look.wgsl`'s `Grade` reads them, three vectors of four floats.

use std::cell::RefCell;

use crate::looks::Grade;

/// The floats `look.wgsl`'s `Grade` holds.
const GRADE_FLOATS: usize = 12;

/// The app's device, and what draws a picture through a look on it.
pub(crate) struct LookPainter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    bindings: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    grade: wgpu::Buffer,
    /// The pipeline for each format drawn onto so far.
    pipelines: RefCell<Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>>,
}

impl LookPainter {
    /// A painter on a new device; why not, where WaOS gives the program none.
    pub(crate) async fn new() -> Result<LookPainter, String> {
        let adapter = wasdk_wgpu::instance()
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|error| format!("no WebGPU adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|error| format!("no WebGPU device: {error}"))?;
        Ok(LookPainter::on(device, queue))
    }

    fn on(device: wgpu::Device, queue: wgpu::Queue) -> LookPainter {
        let shader = device.create_shader_module(wgpu::include_wgsl!("look.wgsl"));
        let bindings = bindings_layout(&device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("look"),
            bind_group_layouts: &[Some(&bindings)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let grade = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grade"),
            size: (GRADE_FLOATS * size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        LookPainter {
            device,
            queue,
            shader,
            bindings,
            layout,
            sampler,
            grade,
            pipelines: RefCell::new(Vec::new()),
        }
    }

    /// The device pictures are drawn on, which the textures drawn from and onto are imported on.
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Draws `picture` through `grade` over the whole of `onto`, shown as `shown` says, and submits
    /// it: `onto` is of the picture's shape turned so, at any size.
    pub(crate) fn paint(
        &self,
        picture: &wgpu::Texture,
        onto: &wgpu::Texture,
        grade: Grade,
        shown: Shown,
    ) {
        let aspect = picture.width() as f32 / picture.height().max(1) as f32;
        self.queue
            .write_buffer(&self.grade, 0, &grade_bytes(grade, shown, aspect));
        let bound = self.bound(picture);
        let pipeline = self.pipeline_onto(onto.format());
        let target = onto.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("look"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bound, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// `picture`, its sampler and the grade, as the shader reads them.
    fn bound(&self, picture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = picture.create_view(&wgpu::TextureViewDescriptor::default());
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("look"),
            layout: &self.bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.grade.as_entire_binding(),
                },
            ],
        })
    }

    /// The pipeline that draws onto a texture in `format`, made the first time one is drawn onto.
    fn pipeline_onto(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut pipelines = self.pipelines.borrow_mut();
        if let Some((_, pipeline)) = pipelines.iter().find(|(made, _)| *made == format) {
            return pipeline.clone();
        }
        let pipeline = self.new_pipeline(format);
        pipelines.push((format, pipeline.clone()));
        pipeline
    }

    fn new_pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("look"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("corner"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("graded"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
    }
}

/// What the shader binds: the picture, its sampler and the grade.
fn bindings_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("look"),
        entries: &[
            entry(
                0,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(
                1,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
            entry(
                2,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
        ],
    })
}

/// How a picture is shown, as a camera's frame says it: turned `rotation` degrees clockwise, then
/// mirrored left to right where `flip`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) rotation: u32,
    pub(crate) flip: bool,
}

impl Shown {
    /// A picture of `size` as it is shown: its sides swapped by a quarter turn.
    pub(crate) fn size_of(self, [width, height]: [u32; 2]) -> [u32; 2] {
        if self.rotation % 180 == 90 {
            [height, width]
        } else {
            [width, height]
        }
    }
}

/// `grade` as the shader's `Grade` lies in memory, for a picture `aspect` times as wide as high,
/// shown as `shown` says.
fn grade_bytes(grade: Grade, shown: Shown, aspect: f32) -> Vec<u8> {
    let [red, green, blue] = grade.tint;
    let floats: [f32; GRADE_FLOATS] = [
        red,
        green,
        blue,
        grade.saturation,
        grade.contrast,
        grade.softness,
        grade.vignette,
        f32::from(u8::from(shown.flip)),
        aspect,
        (shown.rotation % 360 / 90) as f32,
        0.0,
        0.0,
    ];
    floats
        .iter()
        .flat_map(|float| float.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grade_lies_as_three_vectors_of_four_floats() {
        let grade = Grade {
            tint: [1.5, 1.0, 0.5],
            saturation: 0.25,
            contrast: 0.75,
            softness: 2.0,
            vignette: 3.0,
        };
        let shown = Shown {
            rotation: 270,
            flip: true,
        };
        let bytes = grade_bytes(grade, shown, 4.0);
        let (floats, rest) = bytes.as_chunks::<4>();
        let floats: Vec<f32> = floats
            .iter()
            .map(|float| f32::from_le_bytes(*float))
            .collect();
        assert!(rest.is_empty());
        assert_eq!(
            floats,
            [1.5, 1.0, 0.5, 0.25, 0.75, 2.0, 3.0, 1.0, 4.0, 3.0, 0.0, 0.0]
        );
    }

    #[test]
    fn a_picture_turned_a_quarter_shows_its_sides_swapped() {
        let quarter = Shown {
            rotation: 90,
            flip: false,
        };
        assert_eq!(quarter.size_of([640, 480]), [480, 640]);
        assert_eq!(Shown::default().size_of([640, 480]), [640, 480]);
    }
}
