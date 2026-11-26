//! What makes the frame of a surface configured `opaque` opaque: one pass over the frame that
//! writes alpha 1 into every pixel and leaves its colour as drawn, as Chrome's
//! `WebGPUTextureAlphaClearer` does for a canvas configured `opaque`. Telling the system's
//! compositor a layer is opaque does not make it so: Apple's shows the alpha of what a layer's
//! contents hold whatever the layer's `opaque` says, so a frame cleared with alpha 0 would show
//! what is beneath it, the desktop or the program's own layers.
//!
//! The invariant: the pass runs on the program's device, through the frame's own texture, which
//! the canvas makes with `RENDER_ATTACHMENT` for an opaque surface whatever the program asked, and
//! is submitted after the program's work on the frame and before the frame is handed to the
//! compositor (`os::hand_to_compositor`), inside the same allowance to submit.

use std::borrow::Cow;

use anyhow::{Context, Result};
use waos_gpu_host::GpuObject;

use crate::gpu::{GpuInstance, made, wgpu_core, wgpu_types};
use crate::os::FORMAT;

use wgpu_core::command::{RenderPassColorAttachment, RenderPassDescriptor};
use wgpu_core::id;
use wgpu_core::pipeline::{
    FragmentState, ProgrammableStageDescriptor, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderModuleSource, VertexState,
};
use wgpu_core::resource::TextureViewDescriptor;

/// What labels the pass's objects in a GPU debugger.
const LABEL: Option<Cow<'static, str>> = Some(Cow::Borrowed("waos opaque frame"));

/// The pipeline that writes alpha 1 into a frame of the hardware buffers' format, on one device.
pub(super) struct AlphaClearer {
    device: id::DeviceId,
    pipeline: GpuObject<id::RenderPipelineId>,
    _shader: GpuObject<id::ShaderModuleId>,
}

impl AlphaClearer {
    /// The pipeline made on `device`.
    pub(super) fn new(gpu: &GpuInstance, device: id::DeviceId) -> Result<AlphaClearer> {
        let (shader, error) = gpu.device_create_shader_module(
            device,
            &ShaderModuleDescriptor {
                label: LABEL,
                runtime_checks: Default::default(),
            },
            ShaderModuleSource::Wgsl(Cow::Borrowed(include_str!("opaque.wgsl"))),
            None,
        );
        let shader = made(gpu, (shader, error))?;
        let pipeline = made(gpu, alpha_pipeline(gpu, device, shader.id()))?;
        Ok(AlphaClearer {
            device,
            pipeline,
            _shader: shader,
        })
    }

    /// Whether the pipeline is on `device`.
    pub(super) fn is_on(&self, device: id::DeviceId) -> bool {
        self.device == device
    }

    /// Writes alpha 1 into every pixel of `texture`, submitted now on `queue`, after everything
    /// submitted on it before.
    pub(super) fn make_opaque(
        &self,
        gpu: &GpuInstance,
        queue: id::QueueId,
        texture: id::TextureId,
    ) -> Result<()> {
        let view = made(
            gpu,
            gpu.texture_create_view(
                texture,
                &TextureViewDescriptor {
                    label: LABEL,
                    format: Some(FORMAT),
                    dimension: None,
                    usage: Some(wgpu_types::TextureUsages::RENDER_ATTACHMENT),
                    range: wgpu_types::ImageSubresourceRange::default(),
                },
                None,
            ),
        )?;
        let encoder = made(
            gpu,
            gpu.device_create_command_encoder(
                self.device,
                &wgpu_types::CommandEncoderDescriptor { label: LABEL },
                None,
            ),
        )?;
        self.encode(gpu, encoder.id(), view.id())?;
        let (commands, error) = gpu.command_encoder_finish(
            encoder.id(),
            &wgpu_types::CommandBufferDescriptor { label: LABEL },
            None,
        );
        let commands = GpuObject::new(commands, gpu);
        if let Some((_, error)) = error {
            return Err(error).context("encoding an opaque frame's alpha");
        }
        gpu.queue_submit(queue, &[commands.id()])
            .map_err(|(_, error)| error)
            .context("submitting an opaque frame's alpha")?;
        Ok(())
    }

    /// The pass over `target` that keeps what is drawn and writes its alpha, on `encoder`.
    fn encode(
        &self,
        gpu: &GpuInstance,
        encoder: id::CommandEncoderId,
        target: id::TextureViewId,
    ) -> Result<()> {
        let attachment = RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            load_op: wgpu_types::LoadOp::Load,
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
            return Err(error).context("beginning an opaque frame's pass");
        }
        gpu.render_pass_set_pipeline(&mut pass, self.pipeline.id())?;
        gpu.render_pass_draw(&mut pass, 3, 1, 0, 0)?;
        gpu.render_pass_end(&mut pass)?;
        Ok(())
    }
}

/// The pipeline that draws one triangle over the target with `shader`, writing alpha alone, into
/// a target in the format of a hardware buffer; its layout is the shader's, which binds nothing.
fn alpha_pipeline(
    gpu: &GpuInstance,
    device: id::DeviceId,
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
        blend: None,
        write_mask: wgpu_types::ColorWrites::ALPHA,
    };
    gpu.device_create_render_pipeline(
        device,
        &RenderPipelineDescriptor {
            label: LABEL,
            layout: None,
            vertex: VertexState {
                stage: stage("corner"),
                buffers: Cow::Borrowed(&[]),
            },
            primitive: wgpu_types::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu_types::MultisampleState::default(),
            fragment: Some(FragmentState {
                stage: stage("opaque"),
                targets: Cow::Owned(vec![Some(target)]),
            }),
            multiview_mask: None,
            cache: None,
        },
        None,
    )
}
