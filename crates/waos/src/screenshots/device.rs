//! The GPU device screenshots are drawn on: opened on the engine's instance the first time one is
//! taken, with one pipeline, which draws a picture over its quad (`quads`), cut by its clips, and
//! blends it over what is beneath it with its alpha premultiplied, as the system's compositor
//! does. Each picture is sampled through a texture over its hardware buffer's memory, with no copy,
//! and the screenshot is drawn into a hardware buffer of its own, whose pixels are then read.
//!
//! The invariant: every object is a `GpuObject`, given back to the instance as it goes, and a
//! draw waits for its GPU work, so the pictures it read may go once it returns.

use std::borrow::Cow;

use anyhow::{Context, Result, anyhow};
use waos_webgpu_host::{GpuObject, GpuObjectId};

use super::quads::{Quad, bytes_of};
use crate::BufferKind;
use crate::gpu::{GpuInstance, open_device, wgpu_core, wgpu_types};
use crate::os::{FORMAT, HardwareBufferMemory};

use wgpu_core::binding_model::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindingResource, BufferBinding,
    PipelineLayoutDescriptor,
};
use wgpu_core::command::{RenderPassColorAttachment, RenderPassDescriptor};
use wgpu_core::id;
use wgpu_core::pipeline::{
    FragmentState, ProgrammableStageDescriptor, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderModuleSource, VertexState,
};
use wgpu_core::resource::{SamplerDescriptor, TextureViewDescriptor};

/// What labels the device's objects in a GPU debugger.
const LABEL: Option<Cow<'static, str>> = Some(Cow::Borrowed("waos screenshot"));

/// The device, its queue, and what draws a picture on it.
pub(super) struct ScreenshotDevice {
    pipeline: GpuObject<id::RenderPipelineId>,
    _pipeline_layout: GpuObject<id::PipelineLayoutId>,
    _shader: GpuObject<id::ShaderModuleId>,
    layout: GpuObject<id::BindGroupLayoutId>,
    sampler: GpuObject<id::SamplerId>,
    queue: GpuObject<id::QueueId>,
    device: GpuObject<id::DeviceId>,
    _adapter: GpuObject<id::AdapterId>,
}

/// One picture's draw: its bind group, and what the group binds.
struct PictureDraw {
    bind_group: GpuObject<id::BindGroupId>,
    _view: GpuObject<id::TextureViewId>,
    _texture: GpuObject<id::TextureId>,
    _quad: GpuObject<id::BufferId>,
}

impl ScreenshotDevice {
    /// A device on `gpu`'s default adapter, opened as a program's is, with the pipeline made.
    pub(super) fn open(
        gpu: &GpuInstance,
        hardware_buffers: BufferKind,
    ) -> Result<ScreenshotDevice> {
        let adapter = gpu
            .request_adapter(
                &wgpu_types::RequestAdapterOptions::default(),
                wgpu_types::Backends::all(),
                None,
            )
            .context("no GPU adapter for screenshots")?;
        let adapter = GpuObject::new(adapter, gpu);
        let (device, queue) = open_device(
            gpu,
            adapter.id(),
            &wgpu_types::DeviceDescriptor::default(),
            hardware_buffers,
        )
        .context("no GPU device for screenshots")?;
        let (device, queue) = (GpuObject::new(device, gpu), GpuObject::new(queue, gpu));
        let shader = made(
            gpu,
            gpu.device_create_shader_module(
                device.id(),
                &ShaderModuleDescriptor {
                    label: LABEL,
                    runtime_checks: Default::default(),
                },
                ShaderModuleSource::Wgsl(Cow::Borrowed(include_str!("picture.wgsl"))),
                None,
            ),
        )?;
        let layout = made(gpu, bind_group_layout(gpu, device.id()))?;
        let pipeline_layout = made(
            gpu,
            gpu.device_create_pipeline_layout(
                device.id(),
                &PipelineLayoutDescriptor {
                    label: LABEL,
                    bind_group_layouts: Cow::Owned(vec![Some(layout.id())]),
                    immediate_size: 0,
                },
                None,
            ),
        )?;
        let pipeline = made(
            gpu,
            render_pipeline(gpu, device.id(), pipeline_layout.id(), shader.id()),
        )?;
        let sampler = made(gpu, sampler(gpu, device.id()))?;
        Ok(ScreenshotDevice {
            pipeline,
            _pipeline_layout: pipeline_layout,
            _shader: shader,
            layout,
            sampler,
            queue,
            device,
            _adapter: adapter,
        })
    }

    /// Draws `pictures` into `target`, the first beneath, over nothing, each a quad and the memory
    /// its picture lies in, and waits for the GPU to finish.
    pub(super) fn draw(
        &self,
        gpu: &GpuInstance,
        target: &HardwareBufferMemory,
        pictures: &[(Quad, &HardwareBufferMemory)],
    ) -> Result<()> {
        let size = [target.width(), target.height()];
        let target_texture = target.texture_over(
            gpu,
            self.device.id(),
            wgpu_types::TextureUsages::RENDER_ATTACHMENT,
            &[],
        )?;
        let target_texture = GpuObject::new(target_texture, gpu);
        let target_view = self.view_of(gpu, &target_texture)?;
        let draws = pictures
            .iter()
            .map(|(quad, memory)| self.picture_draw(gpu, quad, memory, size))
            .collect::<Result<Vec<_>>>()?;
        let encoder = made(
            gpu,
            gpu.device_create_command_encoder(
                self.device.id(),
                &wgpu_types::CommandEncoderDescriptor { label: LABEL },
                None,
            ),
        )?;
        self.encode(gpu, encoder.id(), target_view.id(), &draws)?;
        let readback =
            target.encode_readback(gpu, self.device.id(), encoder.id(), target_texture.id())?;
        let (commands, error) = gpu.command_encoder_finish(
            encoder.id(),
            &wgpu_types::CommandBufferDescriptor { label: LABEL },
            None,
        );
        let commands = GpuObject::new(commands, gpu);
        if let Some((_, error)) = error {
            return Err(error).context("encoding a screenshot");
        }
        let submission = gpu
            .queue_submit(self.queue.id(), &[commands.id()])
            .map_err(|(_, error)| error)
            .context("submitting a screenshot")?;
        gpu.device_poll(
            self.device.id(),
            wgpu_types::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            },
        )
        .context("waiting for a screenshot to be drawn")?;
        if let Some(finish) = readback {
            finish(gpu);
        }
        Ok(())
    }

    /// The render pass of `draws` into `target`, cleared first, on `encoder`.
    fn encode(
        &self,
        gpu: &GpuInstance,
        encoder: id::CommandEncoderId,
        target: id::TextureViewId,
        draws: &[PictureDraw],
    ) -> Result<()> {
        let attachment = RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            load_op: wgpu_types::LoadOp::Clear(wgpu_types::Color::TRANSPARENT),
            store_op: wgpu_types::StoreOp::Store,
        };
        let (mut pass, error) = gpu.command_encoder_begin_render_pass(
            encoder,
            &RenderPassDescriptor {
                label: LABEL,
                color_attachments: Cow::Owned(vec![Some(attachment)]),
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            },
        );
        if let Some(error) = error {
            return Err(error).context("beginning a screenshot's pass");
        }
        gpu.render_pass_set_pipeline(&mut pass, self.pipeline.id())?;
        for draw in draws {
            gpu.render_pass_set_bind_group(&mut pass, 0, Some(draw.bind_group.id()), &[])?;
            gpu.render_pass_draw(&mut pass, 6, 1, 0, 0)?;
        }
        gpu.render_pass_end(&mut pass)?;
        Ok(())
    }

    /// What draws the picture in `memory` over `quad`, in a screenshot of `size` pixels.
    fn picture_draw(
        &self,
        gpu: &GpuInstance,
        quad: &Quad,
        memory: &HardwareBufferMemory,
        size: [u32; 2],
    ) -> Result<PictureDraw> {
        let bytes = bytes_of(quad, size);
        let buffer = made(
            gpu,
            gpu.device_create_buffer(
                self.device.id(),
                &wgpu_types::BufferDescriptor {
                    label: LABEL,
                    size: bytes.len() as u64,
                    usage: wgpu_types::BufferUsages::STORAGE | wgpu_types::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                },
                None,
            ),
        )?;
        gpu.queue_write_buffer(self.queue.id(), buffer.id(), 0, &bytes)?;
        let texture = GpuObject::new(
            memory.sampled_texture(gpu, self.device.id(), self.queue.id())?,
            gpu,
        );
        let view = self.view_of(gpu, &texture)?;
        let entries = [
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: buffer.id(),
                    offset: 0,
                    size: None,
                }),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(view.id()),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(self.sampler.id()),
            },
        ];
        let bind_group = made(
            gpu,
            gpu.device_create_bind_group(
                self.device.id(),
                &BindGroupDescriptor {
                    label: LABEL,
                    layout: self.layout.id(),
                    entries: Cow::Borrowed(&entries),
                },
                None,
            ),
        )?;
        Ok(PictureDraw {
            bind_group,
            _view: view,
            _texture: texture,
            _quad: buffer,
        })
    }

    /// A view of all of `texture`.
    fn view_of(
        &self,
        gpu: &GpuInstance,
        texture: &GpuObject<id::TextureId>,
    ) -> Result<GpuObject<id::TextureViewId>> {
        made(
            gpu,
            gpu.texture_create_view(
                texture.id(),
                &TextureViewDescriptor {
                    label: LABEL,
                    format: None,
                    dimension: None,
                    usage: None,
                    range: wgpu_types::ImageSubresourceRange::default(),
                },
                None,
            ),
        )
    }
}

/// What a picture's draw binds: its quad, its picture and how the picture is sampled.
fn bind_group_layout(
    gpu: &GpuInstance,
    device: id::DeviceId,
) -> (
    id::BindGroupLayoutId,
    Option<wgpu_core::binding_model::CreateBindGroupLayoutError>,
) {
    let entries = [
        wgpu_types::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu_types::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu_types::BindingType::Buffer {
                ty: wgpu_types::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu_types::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu_types::ShaderStages::FRAGMENT,
            ty: wgpu_types::BindingType::Texture {
                sample_type: wgpu_types::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu_types::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu_types::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu_types::ShaderStages::FRAGMENT,
            ty: wgpu_types::BindingType::Sampler(wgpu_types::SamplerBindingType::Filtering),
            count: None,
        },
    ];
    gpu.device_create_bind_group_layout(
        device,
        &BindGroupLayoutDescriptor {
            label: LABEL,
            entries: Cow::Borrowed(&entries),
        },
        None,
    )
}

/// The pipeline that draws one picture with `shader`, blending it over what is beneath it as
/// premultiplied pixels are blended, into a target in the format of a hardware buffer.
fn render_pipeline(
    gpu: &GpuInstance,
    device: id::DeviceId,
    layout: id::PipelineLayoutId,
    shader: id::ShaderModuleId,
) -> (
    id::RenderPipelineId,
    Option<wgpu_core::pipeline::CreateRenderPipelineError>,
) {
    let stage = |entry_point: &'static str| ProgrammableStageDescriptor {
        module: shader,
        entry_point: Some(Cow::Borrowed(entry_point)),
        constants: Default::default(),
        zero_initialize_workgroup_memory: false,
    };
    let target = wgpu_types::ColorTargetState {
        format: FORMAT,
        blend: Some(wgpu_types::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        write_mask: wgpu_types::ColorWrites::ALL,
    };
    gpu.device_create_render_pipeline(
        device,
        &RenderPipelineDescriptor {
            label: LABEL,
            layout: Some(layout),
            vertex: VertexState {
                stage: stage("corner"),
                buffers: Cow::Borrowed(&[]),
            },
            primitive: wgpu_types::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu_types::MultisampleState::default(),
            fragment: Some(FragmentState {
                stage: stage("shade"),
                targets: Cow::Owned(vec![Some(target)]),
            }),
            multiview_mask: None,
            cache: None,
        },
        None,
    )
}

/// How a picture is sampled: smoothly where it is scaled, its edges held at its edge.
fn sampler(
    gpu: &GpuInstance,
    device: id::DeviceId,
) -> (
    id::SamplerId,
    Option<wgpu_core::resource::CreateSamplerError>,
) {
    gpu.device_create_sampler(
        device,
        &SamplerDescriptor {
            label: LABEL,
            address_modes: [wgpu_types::AddressMode::ClampToEdge; 3],
            mag_filter: wgpu_types::FilterMode::Linear,
            min_filter: wgpu_types::FilterMode::Linear,
            mipmap_filter: wgpu_types::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        },
        None,
    )
}

/// The object wgpu-core `made`, held so it is given back as it goes, or its error: an object made
/// with an error is invalid, and given back all the same.
fn made<Id: GpuObjectId, E: std::error::Error + Send + Sync + 'static>(
    gpu: &GpuInstance,
    (id, error): (Id, Option<E>),
) -> Result<GpuObject<Id>> {
    let object = GpuObject::new(id, gpu);
    match error {
        Some(error) => Err(anyhow!(error)),
        None => Ok(object),
    }
}
