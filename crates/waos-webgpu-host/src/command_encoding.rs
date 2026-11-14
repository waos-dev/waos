//! Recording commands: a command encoder, the render and compute passes begun on it, and render
//! bundle encoders.
//!
//! Each keeps wgpu-core's own encoder, which knows whether it is open or has ended, and every call
//! records through [`WasiWebGpuCtx::record`], which hands what wgpu-core refuses to the encoder's
//! device. The invariant: a call on an encoder is never a trap or a panic. One on an encoder that
//! has ended, a pass after `end` or an encoder after `finish`, is WebGPU's validation error on the
//! device: https://www.w3.org/TR/webgpu/#abstract-opdef-validate-the-encoder-state

use std::fmt::Display;
use std::num::NonZeroU64;
use std::sync::Arc;

use wasmtime::component::Resource;
use wgpu_core::global::Global;
use wgpu_types::error::WebGpuError;

use crate::gpu_object::GpuObject;
use crate::labels::ObjectLabel;
use crate::to_core_conversions::ToCore;
use crate::types::{
    CommandEncoder, ComputePassEncoder, ErrorHandler, RenderBundleEncoder, RenderPassEncoder,
};
use crate::wasi::webgpu::webgpu;
use crate::WasiWebGpuCtx;

/// An encoder a program records commands into: what wgpu-core records them into, and the
/// device's error handler, where what wgpu-core refuses is reported.
pub(crate) trait Recording: Send + 'static {
    type Commands;

    fn commands_and_errors(&mut self) -> (&mut Self::Commands, &ErrorHandler);
}

impl Recording for CommandEncoder {
    type Commands = GpuObject<wgpu_core::id::CommandEncoderId>;

    fn commands_and_errors(&mut self) -> (&mut Self::Commands, &ErrorHandler) {
        (&mut self.command_encoder, &self.error_handler)
    }
}

impl Recording for RenderPassEncoder {
    type Commands = wgpu_core::command::RenderPass;

    fn commands_and_errors(&mut self) -> (&mut Self::Commands, &ErrorHandler) {
        (&mut self.pass, &self.error_handler)
    }
}

impl Recording for ComputePassEncoder {
    type Commands = wgpu_core::command::ComputePass;

    fn commands_and_errors(&mut self) -> (&mut Self::Commands, &ErrorHandler) {
        (&mut self.pass, &self.error_handler)
    }
}

impl Recording for RenderBundleEncoder {
    type Commands = wgpu_core::command::RenderBundleEncoder;

    fn commands_and_errors(&mut self) -> (&mut Self::Commands, &ErrorHandler) {
        (&mut self.encoder, &self.error_handler)
    }
}

impl WasiWebGpuCtx<'_> {
    /// Records into `encoder` with `record`, and reports what wgpu-core refuses to the encoder's
    /// device.
    fn record<R: Recording, E: WebGpuError + Display>(
        &mut self,
        encoder: &Resource<R>,
        record: impl FnOnce(&Global, &mut R::Commands) -> Result<(), E>,
    ) -> wasmtime::Result<()> {
        let (commands, errors) = self.table.get_mut(encoder)?.commands_and_errors();
        errors.handle_possible_error(record(self.instance, commands).err());
        Ok(())
    }

    /// Reports `message` as a validation error on the device `encoder` records for.
    fn report_on_device_of<R: Recording>(
        &mut self,
        encoder: &Resource<R>,
        message: String,
    ) -> wasmtime::Result<()> {
        let (_, errors) = self.table.get_mut(encoder)?.commands_and_errors();
        errors.report_validation_error(message);
        Ok(())
    }

    /// The buffer and the range of it a `set-vertex-buffer` or `set-index-buffer` binds, as
    /// wgpu-core takes them.
    fn buffer_binding(
        &self,
        buffer: &Resource<webgpu::GpuBuffer>,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<(wgpu_core::id::BufferId, u64, Option<NonZeroU64>)> {
        let buffer = self.table.get(buffer)?;
        let (offset, size) = binding_range(buffer.size, offset.unwrap_or(0), size);
        Ok((buffer.id(), offset, size))
    }

    fn bind_group_id(
        &self,
        bind_group: Option<Resource<webgpu::GpuBindGroup>>,
    ) -> wasmtime::Result<Option<wgpu_core::id::BindGroupId>> {
        bind_group
            .map(|bind_group| Ok(self.table.get(&bind_group)?.id()))
            .transpose()
    }
}

/// The offset and size wgpu-core binds for a vertex or index buffer binding of `size` bytes at
/// `offset`, the rest of the buffer when `size` is none.
///
/// WebGPU binds zero bytes where wgpu-core has no zero size; a binding that starts at the buffer's
/// end is empty in wgpu-core too, so an empty binding is made there, where it binds the same
/// nothing. An offset past the end is kept, for wgpu-core to refuse. One difference remains: at
/// the end of a buffer whose size is not a multiple of the binding's alignment, wgpu-core refuses
/// the empty binding that WebGPU accepts.
fn binding_range(buffer_size: u64, offset: u64, size: Option<u64>) -> (u64, Option<NonZeroU64>) {
    match size.map(NonZeroU64::new) {
        None => (offset, None),
        Some(Some(size)) => (offset, Some(size)),
        Some(None) if offset <= buffer_size => (buffer_size, None),
        Some(None) => (offset, None),
    }
}

/// The dynamic offsets a `set-bind-group` names: `length` of them from `start` in `data`, all of
/// them from `start` when `length` is none, or the `RangeError` WebGPU throws when they fall
/// outside the data.
/// https://www.w3.org/TR/webgpu/#dom-gpubindingcommandsmixin-setbindgroup-index-bindgroup-dynamicoffsetsdata-dynamicoffsetsdatastart-dynamicoffsetsdatalength
fn dynamic_offsets(
    data: &[u32],
    start: Option<webgpu::GpuSize64>,
    length: Option<webgpu::GpuSize32>,
) -> Result<&[u32], webgpu::SetBindGroupError> {
    let out_of_range = || webgpu::SetBindGroupError {
        kind: webgpu::SetBindGroupErrorKind::RangeError,
        message: "the dynamic offsets fall outside their data".to_owned(),
    };
    let start = usize::try_from(start.unwrap_or(0)).map_err(|_| out_of_range())?;
    let from_start = data.get(start..).ok_or_else(out_of_range)?;
    match length {
        Some(length) => from_start.get(..length as usize).ok_or_else(out_of_range),
        None => Ok(from_start),
    }
}

/// The immediate data a `set-immediates` writes: `size` bytes of `data` from `offset`, all of them
/// from `offset` when `size` is none, or the message of the `OperationError` WebGPU throws when
/// they fall outside the data.
/// https://www.w3.org/TR/webgpu/#dom-gpubindingcommandsmixin-setimmediates
fn immediate_data(data: &[u8], offset: Option<u64>, size: Option<u64>) -> Result<&[u8], String> {
    let outside = || "the immediate data's offset and size fall outside the data".to_owned();
    let offset = usize::try_from(offset.unwrap_or(0)).map_err(|_| outside())?;
    let from_offset = data.get(offset..).ok_or_else(outside)?;
    match size {
        Some(size) => usize::try_from(size)
            .ok()
            .and_then(|size| from_offset.get(..size))
            .ok_or_else(outside),
        None => Ok(from_offset),
    }
}

impl<'a> webgpu::HostGpuCommandEncoder for WasiWebGpuCtx<'a> {
    fn begin_render_pass(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        descriptor: webgpu::GpuRenderPassDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuRenderPassEncoder>> {
        let command_encoder = self.table.get(&command_encoder)?;
        let command_encoder_id = command_encoder.id();
        let error_handler = Arc::clone(&command_encoder.error_handler);
        let descriptor = wgpu_core::command::RenderPassDescriptor {
            label: descriptor.label.map(|l| l.into()),
            color_attachments: descriptor
                .color_attachments
                .into_iter()
                .map(|c| c.map(|c| c.to_core(self.table)))
                .collect::<Vec<_>>()
                .into(),
            depth_stencil_attachment: descriptor
                .depth_stencil_attachment
                .map(|d| d.to_core(self.table)),
            timestamp_writes: descriptor.timestamp_writes.map(|tw| tw.to_core(self.table)),
            occlusion_query_set: descriptor
                .occlusion_query_set
                .map(|oqs| oqs.to_core(self.table)),
            // multiview_mask is not present in WebGPU
            multiview_mask: None,
            // TODO: self.max_draw_count not used
        };
        let (pass, err) = self
            .instance
            .command_encoder_begin_render_pass(command_encoder_id, &descriptor);

        error_handler.handle_possible_error(err);

        Ok(self.table.push(RenderPassEncoder {
            pass,
            error_handler,
            label: ObjectLabel::of(descriptor.label.as_deref()),
        })?)
    }

    fn finish(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        descriptor: Option<webgpu::GpuCommandBufferDescriptor>,
    ) -> wasmtime::Result<Resource<webgpu::GpuCommandBuffer>> {
        let command_encoder = self.table.get(&command_encoder)?;
        let command_encoder_id = command_encoder.id();
        let error_handler = Arc::clone(&command_encoder.error_handler);
        let descriptor = descriptor
            .map(|d| d.to_core(self.table))
            .unwrap_or(wgpu_types::CommandBufferDescriptor::default());
        let (command_buffer, err) =
            self.instance
                .command_encoder_finish(command_encoder_id, &descriptor, None);
        // The error comes with the encoder's label, which the message already names.
        let err = err.map(|(_label, err)| err);
        error_handler.handle_possible_error(err);
        Ok(self.table.push(
            GpuObject::new(command_buffer, self.instance).labelled(descriptor.label.as_deref()),
        )?)
    }

    fn begin_compute_pass(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        descriptor: Option<webgpu::GpuComputePassDescriptor>,
    ) -> wasmtime::Result<Resource<webgpu::GpuComputePassEncoder>> {
        let command_encoder = self.table.get(&command_encoder)?;
        let command_encoder_id = command_encoder.id();
        let error_handler = Arc::clone(&command_encoder.error_handler);
        let label = ObjectLabel::of(descriptor.as_ref().and_then(|d| d.label.as_deref()));
        let (pass, err) = self.instance.command_encoder_begin_compute_pass(
            command_encoder_id,
            // can't use to_core because timestamp_writes is Option<&x>.
            &wgpu_core::command::ComputePassDescriptor {
                // TODO: can we get rid of the clone here?
                label: descriptor
                    .as_ref()
                    .and_then(|d| d.label.clone().map(|l| l.into())),
                timestamp_writes: descriptor
                    .and_then(|d| d.timestamp_writes.map(|tw| tw.to_core(self.table))),
            },
        );
        error_handler.handle_possible_error(err);
        Ok(self.table.push(ComputePassEncoder {
            pass,
            error_handler,
            label,
        })?)
    }

    fn copy_buffer_to_buffer(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        source: Resource<webgpu::GpuBuffer>,
        source_offset: Option<webgpu::GpuSize64>,
        destination: Resource<webgpu::GpuBuffer>,
        destination_offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let source = self.table.get(&source)?.id();
        let destination = self.table.get(&destination)?.id();
        // https://www.w3.org/TR/webgpu/#dom-gpucommandencoder-copybuffertobuffer
        // Note: wasi:webgpu uses `option` for offsets in lieu of the shorthand overload
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_copy_buffer_to_buffer(
                encoder.id(),
                source,
                source_offset.unwrap_or(0),
                destination,
                destination_offset.unwrap_or(0),
                size,
            )
        })
    }

    fn copy_buffer_to_texture(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        source: webgpu::GpuTexelCopyBufferInfo,
        destination: webgpu::GpuTexelCopyTextureInfo,
        copy_size: webgpu::GpuExtent3D,
    ) -> wasmtime::Result<()> {
        let source = source.to_core(self.table);
        let destination = destination.to_core(self.table);
        let copy_size = copy_size.to_core(self.table);
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_copy_buffer_to_texture(
                encoder.id(),
                &source,
                &destination,
                &copy_size,
            )
        })
    }

    fn copy_texture_to_buffer(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        source: webgpu::GpuTexelCopyTextureInfo,
        destination: webgpu::GpuTexelCopyBufferInfo,
        copy_size: webgpu::GpuExtent3D,
    ) -> wasmtime::Result<()> {
        let source = source.to_core(self.table);
        let destination = destination.to_core(self.table);
        let copy_size = copy_size.to_core(self.table);
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_copy_texture_to_buffer(
                encoder.id(),
                &source,
                &destination,
                &copy_size,
            )
        })
    }

    fn copy_texture_to_texture(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        source: webgpu::GpuTexelCopyTextureInfo,
        destination: webgpu::GpuTexelCopyTextureInfo,
        copy_size: webgpu::GpuExtent3D,
    ) -> wasmtime::Result<()> {
        let source = source.to_core(self.table);
        let destination = destination.to_core(self.table);
        let copy_size = copy_size.to_core(self.table);
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_copy_texture_to_texture(
                encoder.id(),
                &source,
                &destination,
                &copy_size,
            )
        })
    }

    fn clear_buffer(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        buffer: Resource<webgpu::GpuBuffer>,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        // https://www.w3.org/TR/webgpu/#gpucommandencoder
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_clear_buffer(encoder.id(), buffer, offset.unwrap_or(0), size)
        })
    }

    fn resolve_query_set(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        query_set: Resource<webgpu::GpuQuerySet>,
        first_query: webgpu::GpuSize32,
        query_count: webgpu::GpuSize32,
        destination: Resource<webgpu::GpuBuffer>,
        destination_offset: webgpu::GpuSize64,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        let destination = self.table.get(&destination)?.id();
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_resolve_query_set(
                encoder.id(),
                query_set,
                first_query,
                query_count,
                destination,
                destination_offset,
            )
        })
    }

    fn label(&mut self, object: Resource<CommandEncoder>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<CommandEncoder>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn push_debug_group(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        group_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_push_debug_group(encoder.id(), &group_label)
        })
    }

    fn pop_debug_group(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
    ) -> wasmtime::Result<()> {
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_pop_debug_group(encoder.id())
        })
    }

    fn insert_debug_marker(
        &mut self,
        command_encoder: Resource<CommandEncoder>,
        marker_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&command_encoder, |instance, encoder| {
            instance.command_encoder_insert_debug_marker(encoder.id(), &marker_label)
        })
    }

    fn drop(&mut self, command_encoder: Resource<CommandEncoder>) -> wasmtime::Result<()> {
        self.table.delete(command_encoder)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuRenderPassEncoder for WasiWebGpuCtx<'a> {
    fn set_pipeline(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        pipeline: Resource<webgpu::GpuRenderPipeline>,
    ) -> wasmtime::Result<()> {
        let pipeline = self.table.get(&pipeline)?.id();
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_pipeline(pass, pipeline)
        })
    }

    fn draw(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        vertex_count: webgpu::GpuSize32,
        instance_count: Option<webgpu::GpuSize32>,
        first_vertex: Option<webgpu::GpuSize32>,
        first_instance: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<()> {
        // https://www.w3.org/TR/webgpu/#gpurendercommandsmixin
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_draw(
                pass,
                vertex_count,
                instance_count.unwrap_or(1),
                first_vertex.unwrap_or(0),
                first_instance.unwrap_or(0),
            )
        })
    }

    fn end(&mut self, render_pass: Resource<RenderPassEncoder>) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_end(pass)
        })
    }

    fn set_viewport(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_viewport(pass, x, y, width, height, min_depth, max_depth)
        })
    }

    fn set_scissor_rect(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        x: webgpu::GpuIntegerCoordinate,
        y: webgpu::GpuIntegerCoordinate,
        width: webgpu::GpuIntegerCoordinate,
        height: webgpu::GpuIntegerCoordinate,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_scissor_rect(pass, x, y, width, height)
        })
    }

    fn set_blend_constant(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        color: webgpu::GpuColor,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_blend_constant(pass, color.into())
        })
    }

    fn set_stencil_reference(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        reference: webgpu::GpuStencilValue,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_stencil_reference(pass, reference)
        })
    }

    fn begin_occlusion_query(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        query_index: webgpu::GpuSize32,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_begin_occlusion_query(pass, query_index)
        })
    }

    fn end_occlusion_query(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_end_occlusion_query(pass)
        })
    }

    fn execute_bundles(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        bundles: Vec<Resource<webgpu::GpuRenderBundle>>,
    ) -> wasmtime::Result<()> {
        let bundles = bundles
            .iter()
            .map(|bundle| Ok(self.table.get(bundle)?.id()))
            .collect::<wasmtime::Result<Vec<_>>>()?;
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_execute_bundles(pass, &bundles)
        })
    }

    fn label(&mut self, object: Resource<RenderPassEncoder>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<RenderPassEncoder>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn push_debug_group(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        group_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_push_debug_group(pass, &group_label, 0)
        })
    }

    fn pop_debug_group(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_pop_debug_group(pass)
        })
    }

    fn insert_debug_marker(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        marker_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_insert_debug_marker(pass, &marker_label, 0)
        })
    }

    fn set_bind_group(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        index: webgpu::GpuIndex32,
        bind_group: Option<Resource<webgpu::GpuBindGroup>>,
        dynamic_offsets_data: Option<Vec<webgpu::GpuBufferDynamicOffset>>,
        dynamic_offsets_data_start: Option<webgpu::GpuSize64>,
        dynamic_offsets_data_length: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<Result<(), webgpu::SetBindGroupError>> {
        let bind_group = self.bind_group_id(bind_group)?;
        let data = dynamic_offsets_data.unwrap_or_default();
        let offsets = match dynamic_offsets(
            &data,
            dynamic_offsets_data_start,
            dynamic_offsets_data_length,
        ) {
            Ok(offsets) => offsets,
            Err(error) => return Ok(Err(error)),
        };
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_bind_group(pass, index, bind_group, offsets)
        })?;
        Ok(Ok(()))
    }

    fn set_index_buffer(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        buffer: Resource<webgpu::GpuBuffer>,
        index_format: webgpu::GpuIndexFormat,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let (buffer, offset, size) = self.buffer_binding(&buffer, offset, size)?;
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_index_buffer(pass, buffer, index_format.into(), offset, size)
        })
    }

    fn set_vertex_buffer(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        slot: webgpu::GpuIndex32,
        buffer: Option<Resource<webgpu::GpuBuffer>>,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let (buffer, offset, size) = match buffer {
            Some(buffer) => {
                let (buffer, offset, size) = self.buffer_binding(&buffer, offset, size)?;
                (Some(buffer), offset, size)
            }
            None => (None, offset.unwrap_or(0), size.and_then(NonZeroU64::new)),
        };
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_set_vertex_buffer(pass, slot, buffer, offset, size)
        })
    }

    fn draw_indexed(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        index_count: webgpu::GpuSize32,
        instance_count: Option<webgpu::GpuSize32>,
        first_index: Option<webgpu::GpuSize32>,
        base_vertex: Option<webgpu::GpuSignedOffset32>,
        first_instance: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<()> {
        // https://www.w3.org/TR/webgpu/#gpurendercommandsmixin
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_draw_indexed(
                pass,
                index_count,
                instance_count.unwrap_or(1),
                first_index.unwrap_or(0),
                base_vertex.unwrap_or(0),
                first_instance.unwrap_or(0),
            )
        })
    }

    fn draw_indirect(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        indirect_buffer: Resource<webgpu::GpuBuffer>,
        indirect_offset: webgpu::GpuSize64,
    ) -> wasmtime::Result<()> {
        let indirect_buffer = self.table.get(&indirect_buffer)?.id();
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_draw_indirect(pass, indirect_buffer, indirect_offset)
        })
    }

    fn draw_indexed_indirect(
        &mut self,
        render_pass: Resource<RenderPassEncoder>,
        indirect_buffer: Resource<webgpu::GpuBuffer>,
        indirect_offset: webgpu::GpuSize64,
    ) -> wasmtime::Result<()> {
        let indirect_buffer = self.table.get(&indirect_buffer)?.id();
        self.record(&render_pass, |instance, pass| {
            instance.render_pass_draw_indexed_indirect(pass, indirect_buffer, indirect_offset)
        })
    }

    fn set_immediates(
        &mut self,
        render_pass: Resource<webgpu::GpuRenderPassEncoder>,
        range_offset: u32,
        data: Vec<u8>,
        data_offset: Option<u64>,
        data_size: Option<u64>,
    ) -> wasmtime::Result<()> {
        match immediate_data(&data, data_offset, data_size) {
            Ok(data) => self.record(&render_pass, |instance, pass| {
                instance.render_pass_set_immediates(pass, range_offset, data)
            }),
            Err(message) => self.report_on_device_of(&render_pass, message),
        }
    }

    fn drop(&mut self, render_pass: Resource<RenderPassEncoder>) -> wasmtime::Result<()> {
        self.table.delete(render_pass)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuComputePassEncoder for WasiWebGpuCtx<'a> {
    fn set_pipeline(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        pipeline: Resource<webgpu::GpuComputePipeline>,
    ) -> wasmtime::Result<()> {
        let pipeline = self.table.get(&pipeline)?.id();
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_set_pipeline(pass, pipeline)
        })
    }

    fn dispatch_workgroups(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        workgroup_count_x: webgpu::GpuSize32,
        workgroup_count_y: Option<webgpu::GpuSize32>,
        workgroup_count_z: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<()> {
        // https://www.w3.org/TR/webgpu/#gpucomputepassencoder
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_dispatch_workgroups(
                pass,
                workgroup_count_x,
                workgroup_count_y.unwrap_or(1),
                workgroup_count_z.unwrap_or(1),
            )
        })
    }

    fn dispatch_workgroups_indirect(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        indirect_buffer: Resource<webgpu::GpuBuffer>,
        indirect_offset: webgpu::GpuSize64,
    ) -> wasmtime::Result<()> {
        let indirect_buffer = self.table.get(&indirect_buffer)?.id();
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_dispatch_workgroups_indirect(
                pass,
                indirect_buffer,
                indirect_offset,
            )
        })
    }

    fn end(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
    ) -> wasmtime::Result<()> {
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_end(pass)
        })
    }

    fn label(
        &mut self,
        object: Resource<webgpu::GpuComputePassEncoder>,
    ) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuComputePassEncoder>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn push_debug_group(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        group_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_push_debug_group(pass, &group_label, 0)
        })
    }

    fn pop_debug_group(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
    ) -> wasmtime::Result<()> {
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_pop_debug_group(pass)
        })
    }

    fn insert_debug_marker(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_insert_debug_marker(pass, &label, 0)
        })
    }

    fn set_bind_group(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        index: webgpu::GpuIndex32,
        bind_group: Option<Resource<webgpu::GpuBindGroup>>,
        dynamic_offsets_data: Option<Vec<webgpu::GpuBufferDynamicOffset>>,
        dynamic_offsets_data_start: Option<webgpu::GpuSize64>,
        dynamic_offsets_data_length: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<Result<(), webgpu::SetBindGroupError>> {
        let bind_group = self.bind_group_id(bind_group)?;
        let data = dynamic_offsets_data.unwrap_or_default();
        let offsets = match dynamic_offsets(
            &data,
            dynamic_offsets_data_start,
            dynamic_offsets_data_length,
        ) {
            Ok(offsets) => offsets,
            Err(error) => return Ok(Err(error)),
        };
        self.record(&compute_pass, |instance, pass| {
            instance.compute_pass_set_bind_group(pass, index, bind_group, offsets)
        })?;
        Ok(Ok(()))
    }

    fn set_immediates(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
        range_offset: u32,
        data: Vec<u8>,
        data_offset: Option<u64>,
        data_size: Option<u64>,
    ) -> wasmtime::Result<()> {
        match immediate_data(&data, data_offset, data_size) {
            Ok(data) => self.record(&compute_pass, |instance, pass| {
                instance.compute_pass_set_immediates(pass, range_offset, data)
            }),
            Err(message) => self.report_on_device_of(&compute_pass, message),
        }
    }

    fn drop(
        &mut self,
        compute_pass: Resource<webgpu::GpuComputePassEncoder>,
    ) -> wasmtime::Result<()> {
        self.table.delete(compute_pass)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuRenderBundleEncoder for WasiWebGpuCtx<'a> {
    fn finish(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        descriptor: Option<webgpu::GpuRenderBundleDescriptor>,
    ) -> wasmtime::Result<Resource<webgpu::GpuRenderBundle>> {
        let descriptor = descriptor
            .map(|d| d.to_core(self.table))
            .unwrap_or(wgpu_types::RenderBundleDescriptor::default());
        let bundle_encoder = self.table.get_mut(&bundle_encoder)?;
        let (render_bundle, err) = self.instance.render_bundle_encoder_finish(
            &mut bundle_encoder.encoder,
            &descriptor,
            None,
        );
        bundle_encoder.error_handler.handle_possible_error(err);
        Ok(self.table.push(
            GpuObject::new(render_bundle, self.instance).labelled(descriptor.label.as_deref()),
        )?)
    }

    fn label(
        &mut self,
        object: Resource<webgpu::GpuRenderBundleEncoder>,
    ) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuRenderBundleEncoder>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn push_debug_group(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        group_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_push_debug_group(encoder, &group_label)
        })
    }

    fn pop_debug_group(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
    ) -> wasmtime::Result<()> {
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_pop_debug_group(encoder)
        })
    }

    fn insert_debug_marker(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        marker_label: String,
    ) -> wasmtime::Result<()> {
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_insert_debug_marker(encoder, &marker_label)
        })
    }

    fn set_bind_group(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        index: webgpu::GpuIndex32,
        bind_group: Option<Resource<webgpu::GpuBindGroup>>,
        dynamic_offsets_data: Option<Vec<webgpu::GpuBufferDynamicOffset>>,
        dynamic_offsets_data_start: Option<webgpu::GpuSize64>,
        dynamic_offsets_data_length: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<Result<(), webgpu::SetBindGroupError>> {
        let bind_group = self.bind_group_id(bind_group)?;
        let data = dynamic_offsets_data.unwrap_or_default();
        let offsets = match dynamic_offsets(
            &data,
            dynamic_offsets_data_start,
            dynamic_offsets_data_length,
        ) {
            Ok(offsets) => offsets,
            Err(error) => return Ok(Err(error)),
        };
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_set_bind_group(encoder, index, bind_group, offsets)
        })?;
        Ok(Ok(()))
    }

    fn set_pipeline(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        pipeline: Resource<webgpu::GpuRenderPipeline>,
    ) -> wasmtime::Result<()> {
        let pipeline = self.table.get(&pipeline)?.id();
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_set_pipeline(encoder, pipeline)
        })
    }

    fn set_index_buffer(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        buffer: Resource<webgpu::GpuBuffer>,
        index_format: webgpu::GpuIndexFormat,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let (buffer, offset, size) = self.buffer_binding(&buffer, offset, size)?;
        // https://www.w3.org/TR/webgpu/#gpurendercommandsmixin
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_set_index_buffer(
                encoder,
                buffer,
                index_format.into(),
                offset,
                size,
            )
        })
    }

    fn set_vertex_buffer(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        slot: webgpu::GpuIndex32,
        buffer: Option<Resource<webgpu::GpuBuffer>>,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let (buffer, offset, size) = match buffer {
            Some(buffer) => {
                let (buffer, offset, size) = self.buffer_binding(&buffer, offset, size)?;
                (Some(buffer), offset, size)
            }
            None => (None, offset.unwrap_or(0), size.and_then(NonZeroU64::new)),
        };
        // https://www.w3.org/TR/webgpu/#gpurendercommandsmixin
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_set_vertex_buffer(encoder, slot, buffer, offset, size)
        })
    }

    fn draw(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        vertex_count: webgpu::GpuSize32,
        instance_count: Option<webgpu::GpuSize32>,
        first_vertex: Option<webgpu::GpuSize32>,
        first_instance: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<()> {
        // https://www.w3.org/TR/webgpu/#gpurendercommandsmixin
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_draw(
                encoder,
                vertex_count,
                instance_count.unwrap_or(1),
                first_vertex.unwrap_or(0),
                first_instance.unwrap_or(0),
            )
        })
    }

    fn draw_indexed(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        index_count: webgpu::GpuSize32,
        instance_count: Option<webgpu::GpuSize32>,
        first_index: Option<webgpu::GpuSize32>,
        base_vertex: Option<webgpu::GpuSignedOffset32>,
        first_instance: Option<webgpu::GpuSize32>,
    ) -> wasmtime::Result<()> {
        // https://www.w3.org/TR/webgpu/#gpurendercommandsmixin
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_draw_indexed(
                encoder,
                index_count,
                instance_count.unwrap_or(1),
                first_index.unwrap_or(0),
                base_vertex.unwrap_or(0),
                first_instance.unwrap_or(0),
            )
        })
    }

    fn draw_indirect(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        indirect_buffer: Resource<webgpu::GpuBuffer>,
        indirect_offset: webgpu::GpuSize64,
    ) -> wasmtime::Result<()> {
        let indirect_buffer = self.table.get(&indirect_buffer)?.id();
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_draw_indirect(encoder, indirect_buffer, indirect_offset)
        })
    }

    fn draw_indexed_indirect(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        indirect_buffer: Resource<webgpu::GpuBuffer>,
        indirect_offset: webgpu::GpuSize64,
    ) -> wasmtime::Result<()> {
        let indirect_buffer = self.table.get(&indirect_buffer)?.id();
        self.record(&bundle_encoder, |instance, encoder| {
            instance.render_bundle_encoder_draw_indexed_indirect(
                encoder,
                indirect_buffer,
                indirect_offset,
            )
        })
    }

    fn set_immediates(
        &mut self,
        bundle_encoder: Resource<webgpu::GpuRenderBundleEncoder>,
        range_offset: u32,
        data: Vec<u8>,
        data_offset: Option<u64>,
        data_size: Option<u64>,
    ) -> wasmtime::Result<()> {
        match immediate_data(&data, data_offset, data_size) {
            Ok(data) => self.record(&bundle_encoder, |instance, encoder| {
                instance.render_bundle_encoder_set_immediates(encoder, range_offset, data)
            }),
            Err(message) => self.report_on_device_of(&bundle_encoder, message),
        }
    }

    fn drop(&mut self, encoder: Resource<webgpu::GpuRenderBundleEncoder>) -> wasmtime::Result<()> {
        self.table.delete(encoder)?;
        Ok(())
    }
}
