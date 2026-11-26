//! The command encoder: copies, clears, queries and debug groups recorded for one submission,
//! the passes it begins, and the command buffer it finishes into; and the query sets the
//! commands write.

use std::borrow::Cow;
use std::sync::Arc;

use wasmtime::component::Resource;
use wgpu_core::id::{CommandBufferId, CommandEncoderId, QuerySetId};

use crate::buffers::Buffer;
use crate::conversions::{depth_channel, label, non_zero_u32, stencil_channel};
use crate::devices::Device;
use crate::errors::ErrorSink;
use crate::passes::{ComputePass, RenderPass};
use crate::textures::{Texture, texel_copy_texture};
use crate::wasdk::gpu::gpu::{
    self, ComputePassDescriptor, Extent3d, ImageSubresourceRange, RenderPassDescriptor,
    TexelCopyBufferInfo, TexelCopyTextureInfo, TimestampWrites,
};
use crate::{GpuCtx, GpuObject};

/// A command encoder, with its device's errors.
pub struct CommandEncoder {
    object: GpuObject<CommandEncoderId>,
    label: wgpu_core::Label<'static>,
    errors: Arc<ErrorSink>,
}

pub type CommandBuffer = GpuObject<CommandBufferId>;
pub type QuerySet = GpuObject<QuerySetId>;

impl CommandEncoder {
    /// An encoder labelled `label`, on `device`.
    pub(crate) fn create(device: &Device, label: wgpu_core::Label<'static>) -> CommandEncoder {
        let instance = device.instance();
        let descriptor = wgpu_types::CommandEncoderDescriptor {
            label: label.clone(),
        };
        let (encoder, error) =
            instance.device_create_command_encoder(device.device_id(), &descriptor, None);
        device
            .errors
            .report(error, "Device::create_command_encoder", label.as_deref());
        CommandEncoder {
            object: GpuObject::new(encoder, instance),
            label,
            errors: Arc::clone(&device.errors),
        }
    }

    fn id(&self) -> CommandEncoderId {
        self.object.id()
    }

    fn report<E>(&self, result: Result<(), E>, fn_ident: &'static str)
    where
        E: wgpu_types::error::WebGpuError + std::error::Error + Send + Sync + 'static,
    {
        self.errors
            .report(result.err(), fn_ident, self.label.as_deref());
    }
}

/// A buffer's side of a copy, as wgpu-core takes it.
fn texel_copy_buffer(
    table: &wasmtime::component::ResourceTable,
    info: TexelCopyBufferInfo,
) -> wasmtime::Result<wgpu_types::TexelCopyBufferInfo<wgpu_core::id::BufferId>> {
    Ok(wgpu_types::TexelCopyBufferInfo {
        buffer: table.get(&info.buffer)?.id(),
        layout: info.layout.into(),
    })
}

/// Where a pass writes its timestamps, as wgpu-core takes it.
pub(crate) fn timestamp_writes(
    table: &wasmtime::component::ResourceTable,
    writes: Option<TimestampWrites>,
) -> wasmtime::Result<Option<wgpu_core::command::PassTimestampWrites>> {
    writes
        .map(|writes| {
            Ok(wgpu_core::command::PassTimestampWrites {
                query_set: table.get(&writes.query_set)?.id(),
                beginning_of_pass_write_index: writes.beginning_of_pass_write_index,
                end_of_pass_write_index: writes.end_of_pass_write_index,
            })
        })
        .transpose()
}

impl gpu::HostCommandEncoder for GpuCtx<'_> {
    fn copy_buffer_to_buffer(
        &mut self,
        encoder: Resource<CommandEncoder>,
        source: Resource<Buffer>,
        source_offset: u64,
        destination: Resource<Buffer>,
        destination_offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<()> {
        let (source, destination) = (
            self.table.get(&source)?.id(),
            self.table.get(&destination)?.id(),
        );
        let encoder = self.table.get(&encoder)?;
        let copied = self.instance.command_encoder_copy_buffer_to_buffer(
            encoder.id(),
            source,
            source_offset,
            destination,
            destination_offset,
            size,
        );
        encoder.report(copied, "CommandEncoder::copy_buffer_to_buffer");
        Ok(())
    }

    fn copy_buffer_to_texture(
        &mut self,
        encoder: Resource<CommandEncoder>,
        source: TexelCopyBufferInfo,
        destination: TexelCopyTextureInfo,
        size: Extent3d,
    ) -> wasmtime::Result<()> {
        let source = texel_copy_buffer(self.table, source)?;
        let destination = texel_copy_texture(self.table, destination)?;
        let encoder = self.table.get(&encoder)?;
        let copied = self.instance.command_encoder_copy_buffer_to_texture(
            encoder.id(),
            &source,
            &destination,
            &size.into(),
        );
        encoder.report(copied, "CommandEncoder::copy_buffer_to_texture");
        Ok(())
    }

    fn copy_texture_to_buffer(
        &mut self,
        encoder: Resource<CommandEncoder>,
        source: TexelCopyTextureInfo,
        destination: TexelCopyBufferInfo,
        size: Extent3d,
    ) -> wasmtime::Result<()> {
        let source = texel_copy_texture(self.table, source)?;
        let destination = texel_copy_buffer(self.table, destination)?;
        let encoder = self.table.get(&encoder)?;
        let copied = self.instance.command_encoder_copy_texture_to_buffer(
            encoder.id(),
            &source,
            &destination,
            &size.into(),
        );
        encoder.report(copied, "CommandEncoder::copy_texture_to_buffer");
        Ok(())
    }

    fn copy_texture_to_texture(
        &mut self,
        encoder: Resource<CommandEncoder>,
        source: TexelCopyTextureInfo,
        destination: TexelCopyTextureInfo,
        size: Extent3d,
    ) -> wasmtime::Result<()> {
        let source = texel_copy_texture(self.table, source)?;
        let destination = texel_copy_texture(self.table, destination)?;
        let encoder = self.table.get(&encoder)?;
        let copied = self.instance.command_encoder_copy_texture_to_texture(
            encoder.id(),
            &source,
            &destination,
            &size.into(),
        );
        encoder.report(copied, "CommandEncoder::copy_texture_to_texture");
        Ok(())
    }

    fn clear_texture(
        &mut self,
        encoder: Resource<CommandEncoder>,
        texture: Resource<Texture>,
        range: ImageSubresourceRange,
    ) -> wasmtime::Result<()> {
        let texture = self.table.get(&texture)?.id();
        let encoder = self.table.get(&encoder)?;
        let cleared =
            self.instance
                .command_encoder_clear_texture(encoder.id(), texture, &range.into());
        encoder.report(cleared, "CommandEncoder::clear_texture");
        Ok(())
    }

    fn clear_buffer(
        &mut self,
        encoder: Resource<CommandEncoder>,
        buffer: Resource<Buffer>,
        offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        let encoder = self.table.get(&encoder)?;
        let cleared =
            self.instance
                .command_encoder_clear_buffer(encoder.id(), buffer, offset, size);
        encoder.report(cleared, "CommandEncoder::clear_buffer");
        Ok(())
    }

    fn begin_compute_pass(
        &mut self,
        encoder: Resource<CommandEncoder>,
        descriptor: ComputePassDescriptor,
    ) -> wasmtime::Result<Resource<ComputePass>> {
        let core_descriptor = wgpu_core::command::ComputePassDescriptor {
            label: label(descriptor.label),
            timestamp_writes: timestamp_writes(self.table, descriptor.timestamp_writes)?,
        };
        let encoder = self.table.get(&encoder)?;
        let (pass, error) = self
            .instance
            .command_encoder_begin_compute_pass(encoder.id(), &core_descriptor);
        encoder.errors.report(
            error,
            "CommandEncoder::begin_compute_pass",
            core_descriptor.label.as_deref(),
        );
        let pass = ComputePass::new(pass, self.instance, &encoder.errors);
        Ok(self.table.push(pass)?)
    }

    fn begin_render_pass(
        &mut self,
        encoder: Resource<CommandEncoder>,
        descriptor: RenderPassDescriptor,
    ) -> wasmtime::Result<Resource<RenderPass>> {
        let table = &*self.table;
        let color_attachments = descriptor
            .color_attachments
            .into_iter()
            .map(|attachment| {
                attachment
                    .map(|attachment| -> wasmtime::Result<_> {
                        let (load_op, store_op) = attachment.ops.into();
                        Ok(wgpu_core::command::RenderPassColorAttachment {
                            view: table.get(&attachment.view)?.id(),
                            depth_slice: attachment.depth_slice,
                            resolve_target: attachment
                                .resolve_target
                                .map(|view| Ok::<_, wasmtime::Error>(table.get(&view)?.id()))
                                .transpose()?,
                            load_op,
                            store_op,
                        })
                    })
                    .transpose()
            })
            .collect::<wasmtime::Result<Vec<_>>>()?;
        let depth_stencil_attachment = descriptor
            .depth_stencil_attachment
            .map(|attachment| -> wasmtime::Result<_> {
                Ok(wgpu_core::command::RenderPassDepthStencilAttachment {
                    view: table.get(&attachment.view)?.id(),
                    depth: depth_channel(attachment.depth_ops),
                    stencil: stencil_channel(attachment.stencil_ops),
                })
            })
            .transpose()?;
        let core_descriptor = wgpu_core::command::RenderPassDescriptor {
            label: label(descriptor.label),
            color_attachments: Cow::Owned(color_attachments),
            depth_stencil_attachment,
            timestamp_writes: timestamp_writes(table, descriptor.timestamp_writes)?,
            occlusion_query_set: descriptor
                .occlusion_query_set
                .map(|query_set| Ok::<_, wasmtime::Error>(table.get(&query_set)?.id()))
                .transpose()?,
            multiview_mask: non_zero_u32(
                descriptor.multiview_mask,
                "a render pass's multiview mask",
            )?,
        };
        let encoder = table.get(&encoder)?;
        let (pass, error) = self
            .instance
            .command_encoder_begin_render_pass(encoder.id(), &core_descriptor);
        encoder.errors.report(
            error,
            "CommandEncoder::begin_render_pass",
            core_descriptor.label.as_deref(),
        );
        let pass = RenderPass::new(pass, self.instance, &encoder.errors);
        Ok(self.table.push(pass)?)
    }

    fn write_timestamp(
        &mut self,
        encoder: Resource<CommandEncoder>,
        query_set: Resource<QuerySet>,
        index: u32,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        let encoder = self.table.get(&encoder)?;
        let written = self
            .instance
            .command_encoder_write_timestamp(encoder.id(), query_set, index);
        encoder.report(written, "CommandEncoder::write_timestamp");
        Ok(())
    }

    fn resolve_query_set(
        &mut self,
        encoder: Resource<CommandEncoder>,
        query_set: Resource<QuerySet>,
        first_query: u32,
        query_count: u32,
        destination: Resource<Buffer>,
        destination_offset: u64,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        let destination = self.table.get(&destination)?.id();
        let encoder = self.table.get(&encoder)?;
        let resolved = self.instance.command_encoder_resolve_query_set(
            encoder.id(),
            query_set,
            first_query,
            query_count,
            destination,
            destination_offset,
        );
        encoder.report(resolved, "CommandEncoder::resolve_query_set");
        Ok(())
    }

    fn insert_debug_marker(
        &mut self,
        encoder: Resource<CommandEncoder>,
        marker: String,
    ) -> wasmtime::Result<()> {
        let encoder = self.table.get(&encoder)?;
        let inserted = self
            .instance
            .command_encoder_insert_debug_marker(encoder.id(), &marker);
        encoder.report(inserted, "CommandEncoder::insert_debug_marker");
        Ok(())
    }

    fn push_debug_group(
        &mut self,
        encoder: Resource<CommandEncoder>,
        group: String,
    ) -> wasmtime::Result<()> {
        let encoder = self.table.get(&encoder)?;
        let pushed = self
            .instance
            .command_encoder_push_debug_group(encoder.id(), &group);
        encoder.report(pushed, "CommandEncoder::push_debug_group");
        Ok(())
    }

    fn pop_debug_group(&mut self, encoder: Resource<CommandEncoder>) -> wasmtime::Result<()> {
        let encoder = self.table.get(&encoder)?;
        let popped = self.instance.command_encoder_pop_debug_group(encoder.id());
        encoder.report(popped, "CommandEncoder::pop_debug_group");
        Ok(())
    }

    fn finish(
        &mut self,
        encoder: Resource<CommandEncoder>,
    ) -> wasmtime::Result<Resource<CommandBuffer>> {
        let encoder = self.table.delete(encoder)?;
        let (buffer, error) = self.instance.command_encoder_finish(
            encoder.id(),
            &wgpu_types::CommandBufferDescriptor::default(),
            None,
        );
        if let Some((label, error)) = error {
            encoder
                .errors
                .report(Some(error), "a CommandEncoder", Some(&label));
        }
        Ok(self.table.push(GpuObject::new(buffer, self.instance))?)
    }

    fn drop(&mut self, encoder: Resource<CommandEncoder>) -> wasmtime::Result<()> {
        self.table.delete(encoder)?;
        Ok(())
    }
}

impl gpu::HostCommandBuffer for GpuCtx<'_> {
    fn drop(&mut self, buffer: Resource<CommandBuffer>) -> wasmtime::Result<()> {
        self.table.delete(buffer)?;
        Ok(())
    }
}

impl gpu::HostQuerySet for GpuCtx<'_> {
    fn destroy(&mut self, query_set: Resource<QuerySet>) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        self.instance.query_set_destroy(query_set);
        Ok(())
    }

    fn drop(&mut self, query_set: Resource<QuerySet>) -> wasmtime::Result<()> {
        self.table.delete(query_set)?;
        Ok(())
    }
}
