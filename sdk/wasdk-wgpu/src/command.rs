//! Command encoders and the command buffers they finish into.

use wasdk::webgpu::{
    GpuCommandBuffer, GpuCommandBufferDescriptor, GpuCommandEncoder, GpuCommandEncoderDescriptor,
    GpuDevice,
};
use wgpu::custom::{
    CommandBufferInterface, CommandEncoderInterface, DispatchBuffer, DispatchCommandBuffer,
    DispatchCommandEncoder, DispatchComputePass, DispatchQuerySet, DispatchRenderPass,
    DispatchTexture,
};
use wgpu::{Blas, BufferAddress, Tlas};

use crate::buffer::{WasdkBuffer, texel_copy_buffer_to_wasi};
use crate::gap::wasi_gap;
use crate::query::WasdkQuerySet;
use crate::texture::texel_copy_texture_to_wasi;
use crate::{convert, pass};

#[derive(Debug)]
pub struct WasdkCommandEncoder {
    encoder: GpuCommandEncoder,
    /// The encoder's label, which the command buffer it finishes into inherits.
    label: Option<String>,
}

pub(crate) fn create_command_encoder(
    device: &GpuDevice,
    desc: &wgpu::CommandEncoderDescriptor<'_>,
) -> DispatchCommandEncoder {
    let label = convert::label(desc.label);
    let descriptor = GpuCommandEncoderDescriptor {
        label: label.clone(),
    };
    DispatchCommandEncoder::custom(WasdkCommandEncoder {
        encoder: device.create_command_encoder(Some(&descriptor)),
        label,
    })
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
        self.encoder.copy_buffer_to_buffer(
            WasdkBuffer::of_dispatch(source).gpu_buffer(),
            Some(source_offset),
            WasdkBuffer::of_dispatch(destination).gpu_buffer(),
            Some(destination_offset),
            copy_size,
        );
    }

    fn copy_buffer_to_texture(
        &self,
        source: wgpu::TexelCopyBufferInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        self.encoder.copy_buffer_to_texture(
            &texel_copy_buffer_to_wasi(&source),
            &texel_copy_texture_to_wasi(&destination),
            convert::extent_to_wasi(copy_size),
        );
    }

    fn copy_texture_to_buffer(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyBufferInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        self.encoder.copy_texture_to_buffer(
            &texel_copy_texture_to_wasi(&source),
            &texel_copy_buffer_to_wasi(&destination),
            convert::extent_to_wasi(copy_size),
        );
    }

    fn copy_texture_to_texture(
        &self,
        source: wgpu::TexelCopyTextureInfo<'_>,
        destination: wgpu::TexelCopyTextureInfo<'_>,
        copy_size: wgpu::Extent3d,
    ) {
        self.encoder.copy_texture_to_texture(
            &texel_copy_texture_to_wasi(&source),
            &texel_copy_texture_to_wasi(&destination),
            convert::extent_to_wasi(copy_size),
        );
    }

    fn begin_compute_pass(&self, desc: &wgpu::ComputePassDescriptor<'_>) -> DispatchComputePass {
        pass::begin_compute_pass(&self.encoder, desc)
    }

    fn begin_render_pass(&self, desc: &wgpu::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        pass::begin_render_pass(&self.encoder, desc)
    }

    fn finish(&mut self) -> DispatchCommandBuffer {
        let descriptor = GpuCommandBufferDescriptor {
            label: self.label.clone(),
        };
        DispatchCommandBuffer::custom(WasdkCommandBuffer {
            buffer: self.encoder.finish(Some(&descriptor)),
        })
    }

    fn clear_texture(
        &self,
        _texture: &DispatchTexture,
        _subresource_range: &wgpu::ImageSubresourceRange,
    ) {
        wasi_gap("CommandEncoder::clear_texture")
    }

    fn clear_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: BufferAddress,
        size: Option<BufferAddress>,
    ) {
        self.encoder.clear_buffer(
            WasdkBuffer::of_dispatch(buffer).gpu_buffer(),
            Some(offset),
            size,
        );
    }

    fn insert_debug_marker(&self, label: &str) {
        self.encoder.insert_debug_marker(label);
    }

    fn push_debug_group(&self, label: &str) {
        self.encoder.push_debug_group(label);
    }

    fn pop_debug_group(&self) {
        self.encoder.pop_debug_group();
    }

    fn write_timestamp(&self, _query_set: &DispatchQuerySet, _query_index: u32) {
        wasi_gap(
            "timestamps inside an encoder (CommandEncoder::write_timestamp); WebGPU writes them at pass boundaries",
        )
    }

    fn resolve_query_set(
        &self,
        query_set: &DispatchQuerySet,
        first_query: u32,
        query_count: u32,
        destination: &DispatchBuffer,
        destination_offset: BufferAddress,
    ) {
        self.encoder.resolve_query_set(
            WasdkQuerySet::of_dispatch(query_set).gpu_query_set(),
            first_query,
            query_count,
            WasdkBuffer::of_dispatch(destination).gpu_buffer(),
            destination_offset,
        );
    }

    fn mark_acceleration_structures_built<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a Blas>,
        _tlas: &mut dyn Iterator<Item = &'a Tlas>,
    ) {
        wasi_gap("ray tracing (CommandEncoder::mark_acceleration_structures_built)")
    }

    fn build_acceleration_structures<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a wgpu::BlasBuildEntry<'a>>,
        _tlas: &mut dyn Iterator<Item = &'a Tlas>,
    ) {
        wasi_gap("ray tracing (CommandEncoder::build_acceleration_structures)")
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
        // WebGPU tracks resource state itself; there is nothing to transition.
    }
}

#[derive(Debug)]
pub struct WasdkCommandBuffer {
    buffer: GpuCommandBuffer,
}

downcast!(WasdkCommandBuffer: dispatch DispatchCommandBuffer);

impl WasdkCommandBuffer {
    pub(crate) fn gpu_command_buffer(&self) -> &GpuCommandBuffer {
        &self.buffer
    }
}

impl CommandBufferInterface for WasdkCommandBuffer {}
