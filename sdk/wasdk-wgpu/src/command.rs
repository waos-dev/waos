//! Command encoders and the command buffers they finish into.
//!
//! The invariant: an encoder is the engine's until `finish`, which consumes it, and a command
//! buffer until `submit`, which consumes it; wgpu makes neither call twice.

use std::sync::{Mutex, PoisonError};

use wasdk::gpu::{CommandBuffer, CommandEncoder, Device};
use wgpu::custom::{
    CommandBufferInterface, CommandEncoderInterface, DispatchBuffer, DispatchCommandBuffer,
    DispatchCommandEncoder, DispatchComputePass, DispatchQuerySet, DispatchRenderPass,
    DispatchTexture,
};
use wgpu::{Blas, BufferAddress, Tlas};

use crate::buffer::{WasdkBuffer, texel_copy_buffer_to_wit};
use crate::left_out::left_out;
use crate::query::WasdkQuerySet;
use crate::texture::{WasdkTexture, texel_copy_texture_to_wit};
use crate::{convert, pass};

/// The encoder behind a [`wgpu::CommandEncoder`] from this backend.
#[derive(Debug)]
pub struct WasdkCommandEncoder {
    /// The engine's encoder; none once finished.
    encoder: Option<CommandEncoder>,
}

pub(crate) fn create_command_encoder(
    device: &Device,
    desc: &wgpu::CommandEncoderDescriptor<'_>,
) -> DispatchCommandEncoder {
    DispatchCommandEncoder::custom(WasdkCommandEncoder {
        encoder: Some(device.create_command_encoder(desc.label)),
    })
}

impl WasdkCommandEncoder {
    fn encoder(&self) -> &CommandEncoder {
        self.encoder
            .as_ref()
            .expect("wgpu records nothing into a finished encoder")
    }
}

impl CommandEncoderInterface for WasdkCommandEncoder {
    fn copy_buffer_to_buffer(
        &self,
        source: &DispatchBuffer,
        source_offset: BufferAddress,
        destination: &DispatchBuffer,
        destination_offset: BufferAddress,
        copy_size: Option<BufferAddress>,
    ) {
        self.encoder().copy_buffer_to_buffer(
            WasdkBuffer::of_dispatch(source).wit_buffer(),
            source_offset,
            WasdkBuffer::of_dispatch(destination).wit_buffer(),
            destination_offset,
            copy_size,
        );
    }

    fn copy_buffer_to_texture(
        &self,
        source: wgpu::TexelCopyBufferInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        self.encoder().copy_buffer_to_texture(
            &texel_copy_buffer_to_wit(&source),
            &texel_copy_texture_to_wit(&destination),
            convert::extent_to_wit(copy_size),
        );
    }

    fn copy_texture_to_buffer(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyBufferInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        self.encoder().copy_texture_to_buffer(
            &texel_copy_texture_to_wit(&source),
            &texel_copy_buffer_to_wit(&destination),
            convert::extent_to_wit(copy_size),
        );
    }

    fn copy_texture_to_texture(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        self.encoder().copy_texture_to_texture(
            &texel_copy_texture_to_wit(&source),
            &texel_copy_texture_to_wit(&destination),
            convert::extent_to_wit(copy_size),
        );
    }

    fn begin_compute_pass(&self, desc: &wgpu::ComputePassDescriptor<'_>) -> DispatchComputePass {
        pass::begin_compute_pass(self.encoder(), desc)
    }

    fn begin_render_pass(&self, desc: &wgpu::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        pass::begin_render_pass(self.encoder(), desc)
    }

    fn finish(&mut self) -> DispatchCommandBuffer {
        let encoder = self.encoder.take().expect("wgpu finishes an encoder once");
        DispatchCommandBuffer::custom(WasdkCommandBuffer {
            buffer: Mutex::new(Some(CommandEncoder::finish(encoder))),
        })
    }

    fn clear_texture(
        &self,
        texture: &DispatchTexture,
        subresource_range: &wgpu::ImageSubresourceRange,
    ) {
        self.encoder().clear_texture(
            WasdkTexture::of_dispatch(texture).wit_texture(),
            convert::image_subresource_range_to_wit(subresource_range),
        );
    }

    fn clear_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: BufferAddress,
        size: Option<BufferAddress>,
    ) {
        self.encoder()
            .clear_buffer(WasdkBuffer::of_dispatch(buffer).wit_buffer(), offset, size);
    }

    fn insert_debug_marker(&self, label: &str) {
        self.encoder().insert_debug_marker(label);
    }

    fn push_debug_group(&self, label: &str) {
        self.encoder().push_debug_group(label);
    }

    fn pop_debug_group(&self) {
        self.encoder().pop_debug_group();
    }

    fn write_timestamp(&self, query_set: &DispatchQuerySet, query_index: u32) {
        self.encoder().write_timestamp(
            WasdkQuerySet::of_dispatch(query_set).wit_query_set(),
            query_index,
        );
    }

    fn resolve_query_set(
        &self,
        query_set: &DispatchQuerySet,
        first_query: u32,
        query_count: u32,
        destination: &DispatchBuffer,
        destination_offset: BufferAddress,
    ) {
        self.encoder().resolve_query_set(
            WasdkQuerySet::of_dispatch(query_set).wit_query_set(),
            first_query,
            query_count,
            WasdkBuffer::of_dispatch(destination).wit_buffer(),
            destination_offset,
        );
    }

    fn mark_acceleration_structures_built<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a Blas>,
        _tlas: &mut dyn Iterator<Item = &'a Tlas>,
    ) {
        left_out("ray tracing (CommandEncoder::mark_acceleration_structures_built)")
    }

    fn build_acceleration_structures<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a wgpu::BlasBuildEntry<'a>>,
        _tlas: &mut dyn Iterator<Item = &'a Tlas>,
    ) {
        left_out("ray tracing (CommandEncoder::build_acceleration_structures)")
    }

    fn transition_resources<'a>(
        &mut self,
        _buffer_transitions: &mut dyn Iterator<
            Item = wgpu::wgt::BufferTransition<&'a DispatchBuffer>,
        >,
        _texture_transitions: &mut dyn Iterator<
            Item = wgpu::wgt::TextureTransition<&'a DispatchTexture>,
        >,
    ) {
        // wgpu-core tracks resource state itself; there is nothing to transition.
    }
}

/// The command buffer behind a [`wgpu::CommandBuffer`] from this backend.
#[derive(Debug)]
pub struct WasdkCommandBuffer {
    // Taken by `submit`, which wgpu hands the buffer to by reference only.
    buffer: Mutex<Option<CommandBuffer>>,
}

downcast!(WasdkCommandBuffer: dispatch DispatchCommandBuffer);

impl WasdkCommandBuffer {
    /// The engine's command buffer, for the submission that consumes it.
    pub(crate) fn take(&self) -> CommandBuffer {
        self.buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .expect("wgpu submits a command buffer once")
    }
}

impl CommandBufferInterface for WasdkCommandBuffer {}
