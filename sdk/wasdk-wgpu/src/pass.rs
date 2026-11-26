//! Render passes, compute passes, and render bundles: what draws are recorded into.
//!
//! A pass is the engine's until it is dropped, which ends it, as dropping a wgpu pass does.

use wasdk::gpu::{
    BindGroup, Buffer, CommandEncoder, ComputePass, ComputePassDescriptor, Device, RenderBundle,
    RenderBundleEncoder, RenderPass, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, TimestampWrites,
};
use wasdk::gpu_types::{RenderBundleDepthStencil, RenderBundleEncoderDescriptor};
use wgpu::custom::{
    ComputePassInterface, DispatchBindGroup, DispatchBuffer, DispatchComputePass,
    DispatchComputePipeline, DispatchQuerySet, DispatchRenderBundle, DispatchRenderBundleEncoder,
    DispatchRenderPass, DispatchRenderPipeline, DispatchTextureView, RenderBundleEncoderInterface,
    RenderBundleInterface, RenderPassInterface,
};
use wgpu::{BufferAddress, BufferSize, Color, DynamicOffset, IndexFormat};

use crate::binding::WasdkBindGroup;
use crate::buffer::WasdkBuffer;
use crate::convert;
use crate::left_out::left_out;
use crate::pipeline::{WasdkComputePipeline, WasdkRenderPipeline};
use crate::query::WasdkQuerySet;
use crate::texture::WasdkTextureView;

fn bind_group(group: Option<&DispatchBindGroup>) -> Option<&BindGroup> {
    group.map(|group| WasdkBindGroup::of_dispatch(group).wit_bind_group())
}

fn buffer(buffer: &DispatchBuffer) -> &Buffer {
    WasdkBuffer::of_dispatch(buffer).wit_buffer()
}

fn wit_query_set(query_set: &DispatchQuerySet) -> &wasdk::gpu::QuerySet {
    WasdkQuerySet::of_dispatch(query_set).wit_query_set()
}

/// The timestamp writes of a pass, which wgpu describes alike for both kinds.
fn timestamp_writes<'a>(
    query_set: &'a wgpu::QuerySet,
    beginning_of_pass_write_index: Option<u32>,
    end_of_pass_write_index: Option<u32>,
) -> TimestampWrites<'a> {
    TimestampWrites {
        query_set: WasdkQuerySet::of(query_set).wit_query_set(),
        beginning_of_pass_write_index,
        end_of_pass_write_index,
    }
}

// Render passes.

/// The pass behind a [`wgpu::RenderPass`] from this backend.
#[derive(Debug)]
pub struct WasdkRenderPass {
    pass: RenderPass,
}

pub(crate) fn begin_render_pass(
    encoder: &CommandEncoder,
    desc: &wgpu::RenderPassDescriptor<'_>,
) -> DispatchRenderPass {
    let descriptor = RenderPassDescriptor {
        label: convert::label(desc.label),
        color_attachments: (desc.color_attachments.iter())
            .map(|attachment| attachment.as_ref().map(color_attachment_to_wit))
            .collect(),
        depth_stencil_attachment: (desc.depth_stencil_attachment.as_ref())
            .map(depth_stencil_attachment_to_wit),
        timestamp_writes: desc.timestamp_writes.as_ref().map(|writes| {
            timestamp_writes(
                writes.query_set,
                writes.beginning_of_pass_write_index,
                writes.end_of_pass_write_index,
            )
        }),
        occlusion_query_set: (desc.occlusion_query_set)
            .map(|query_set| WasdkQuerySet::of(query_set).wit_query_set()),
        multiview_mask: desc.multiview_mask.map(std::num::NonZeroU32::get),
    };
    DispatchRenderPass::custom(WasdkRenderPass {
        pass: encoder.begin_render_pass(&descriptor),
    })
}

fn color_attachment_to_wit<'a>(
    attachment: &wgpu::RenderPassColorAttachment<'a>,
) -> RenderPassColorAttachment<'a> {
    RenderPassColorAttachment {
        view: WasdkTextureView::of(attachment.view).wit_texture_view(),
        depth_slice: attachment.depth_slice,
        resolve_target: (attachment.resolve_target)
            .map(|view| WasdkTextureView::of(view).wit_texture_view()),
        ops: convert::color_operations_to_wit(&attachment.ops),
    }
}

fn depth_stencil_attachment_to_wit<'a>(
    attachment: &wgpu::RenderPassDepthStencilAttachment<'a>,
) -> RenderPassDepthStencilAttachment<'a> {
    RenderPassDepthStencilAttachment {
        view: WasdkTextureView::of(attachment.view).wit_texture_view(),
        depth_ops: attachment
            .depth_ops
            .as_ref()
            .map(convert::depth_operations_to_wit),
        stencil_ops: (attachment.stencil_ops.as_ref()).map(convert::stencil_operations_to_wit),
    }
}

impl RenderPassInterface for WasdkRenderPass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        self.pass
            .set_pipeline(WasdkRenderPipeline::of_dispatch(pipeline).wit_render_pipeline());
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        group: Option<&DispatchBindGroup>,
        offsets: &[DynamicOffset],
    ) {
        self.pass.set_bind_group(index, bind_group(group), offsets);
    }

    fn set_index_buffer(
        &mut self,
        index_buffer: &DispatchBuffer,
        index_format: IndexFormat,
        offset: BufferAddress,
        size: Option<BufferSize>,
    ) {
        self.pass.set_index_buffer(
            buffer(index_buffer),
            convert::index_format_to_wit(index_format),
            offset,
            size.map(BufferSize::get),
        );
    }

    fn set_vertex_buffer(
        &mut self,
        slot: u32,
        vertex_buffer: Option<&DispatchBuffer>,
        offset: BufferAddress,
        size: Option<BufferSize>,
    ) {
        self.pass.set_vertex_buffer(
            slot,
            vertex_buffer.map(buffer),
            offset,
            size.map(BufferSize::get),
        );
    }

    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.pass.set_immediates(offset, data);
    }

    fn set_blend_constant(&mut self, color: Color) {
        self.pass.set_blend_constant(convert::color_to_wit(color));
    }

    fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.pass.set_scissor_rect(x, y, width, height);
    }

    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.pass
            .set_viewport(x, y, width, height, min_depth, max_depth);
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        self.pass.set_stencil_reference(reference);
    }

    fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        self.pass.draw(
            convert::range_to_wit(vertices),
            convert::range_to_wit(instances),
        );
    }

    fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.pass.draw_indexed(
            convert::range_to_wit(indices),
            base_vertex,
            convert::range_to_wit(instances),
        );
    }

    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        left_out("mesh shaders (RenderPass::draw_mesh_tasks)")
    }

    fn draw_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: BufferAddress) {
        self.pass
            .draw_indirect(buffer(indirect_buffer), indirect_offset);
    }

    fn draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
    ) {
        self.pass
            .draw_indexed_indirect(buffer(indirect_buffer), indirect_offset);
    }

    fn draw_mesh_tasks_indirect(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
    ) {
        left_out("mesh shaders (RenderPass::draw_mesh_tasks_indirect)")
    }

    fn multi_draw_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
        count: u32,
    ) {
        self.pass
            .multi_draw_indirect(buffer(indirect_buffer), indirect_offset, count);
    }

    fn multi_draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
        count: u32,
    ) {
        self.pass
            .multi_draw_indexed_indirect(buffer(indirect_buffer), indirect_offset, count);
    }

    fn multi_draw_mesh_tasks_indirect(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
        _count: u32,
    ) {
        left_out("mesh shaders (RenderPass::multi_draw_mesh_tasks_indirect)")
    }

    fn multi_draw_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: BufferAddress,
        max_count: u32,
    ) {
        self.pass.multi_draw_indirect_count(
            buffer(indirect_buffer),
            indirect_offset,
            buffer(count_buffer),
            count_buffer_offset,
            max_count,
        );
    }

    fn multi_draw_indexed_indirect_count(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
        count_buffer: &DispatchBuffer,
        count_buffer_offset: BufferAddress,
        max_count: u32,
    ) {
        self.pass.multi_draw_indexed_indirect_count(
            buffer(indirect_buffer),
            indirect_offset,
            buffer(count_buffer),
            count_buffer_offset,
            max_count,
        );
    }

    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: BufferAddress,
        _max_count: u32,
    ) {
        left_out("mesh shaders (RenderPass::multi_draw_mesh_tasks_indirect_count)")
    }

    fn insert_debug_marker(&mut self, label: &str) {
        self.pass.insert_debug_marker(label);
    }

    fn push_debug_group(&mut self, group_label: &str) {
        self.pass.push_debug_group(group_label);
    }

    fn pop_debug_group(&mut self) {
        self.pass.pop_debug_group();
    }

    fn write_timestamp(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        self.pass
            .write_timestamp(wit_query_set(query_set), query_index);
    }

    fn begin_occlusion_query(&mut self, query_index: u32) {
        self.pass.begin_occlusion_query(query_index);
    }

    fn end_occlusion_query(&mut self) {
        self.pass.end_occlusion_query();
    }

    fn begin_pipeline_statistics_query(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        self.pass
            .begin_pipeline_statistics_query(wit_query_set(query_set), query_index);
    }

    fn end_pipeline_statistics_query(&mut self) {
        self.pass.end_pipeline_statistics_query();
    }

    fn execute_bundles(&mut self, render_bundles: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        let bundles: Vec<&RenderBundle> = render_bundles
            .map(|bundle| WasdkRenderBundle::of_dispatch(bundle).wit_render_bundle())
            .collect();
        self.pass.execute_bundles(&bundles);
    }
}

impl Drop for WasdkRenderPass {
    /// The engine's pass, dropped with this, ends the pass.
    fn drop(&mut self) {}
}

// Compute passes.

/// The pass behind a [`wgpu::ComputePass`] from this backend.
#[derive(Debug)]
pub struct WasdkComputePass {
    pass: ComputePass,
}

pub(crate) fn begin_compute_pass(
    encoder: &CommandEncoder,
    desc: &wgpu::ComputePassDescriptor<'_>,
) -> DispatchComputePass {
    let descriptor = ComputePassDescriptor {
        label: convert::label(desc.label),
        timestamp_writes: desc.timestamp_writes.as_ref().map(|writes| {
            timestamp_writes(
                writes.query_set,
                writes.beginning_of_pass_write_index,
                writes.end_of_pass_write_index,
            )
        }),
    };
    DispatchComputePass::custom(WasdkComputePass {
        pass: encoder.begin_compute_pass(&descriptor),
    })
}

impl ComputePassInterface for WasdkComputePass {
    fn set_pipeline(&mut self, pipeline: &DispatchComputePipeline) {
        self.pass
            .set_pipeline(WasdkComputePipeline::of_dispatch(pipeline).wit_compute_pipeline());
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        group: Option<&DispatchBindGroup>,
        offsets: &[DynamicOffset],
    ) {
        self.pass.set_bind_group(index, bind_group(group), offsets);
    }

    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.pass.set_immediates(offset, data);
    }

    fn insert_debug_marker(&mut self, label: &str) {
        self.pass.insert_debug_marker(label);
    }

    fn push_debug_group(&mut self, group_label: &str) {
        self.pass.push_debug_group(group_label);
    }

    fn pop_debug_group(&mut self) {
        self.pass.pop_debug_group();
    }

    fn write_timestamp(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        self.pass
            .write_timestamp(wit_query_set(query_set), query_index);
    }

    fn begin_pipeline_statistics_query(&mut self, query_set: &DispatchQuerySet, query_index: u32) {
        self.pass
            .begin_pipeline_statistics_query(wit_query_set(query_set), query_index);
    }

    fn end_pipeline_statistics_query(&mut self) {
        self.pass.end_pipeline_statistics_query();
    }

    fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        self.pass.dispatch_workgroups(x, y, z);
    }

    fn dispatch_workgroups_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
    ) {
        self.pass
            .dispatch_workgroups_indirect(buffer(indirect_buffer), indirect_offset);
    }

    fn transition_resources<'a>(
        &mut self,
        _buffer_transitions: &mut dyn Iterator<
            Item = wgpu::wgt::BufferTransition<&'a DispatchBuffer>,
        >,
        _texture_transitions: &mut dyn Iterator<
            Item = wgpu::wgt::TextureTransition<&'a DispatchTextureView>,
        >,
    ) {
        // wgpu-core tracks resource state itself; there is nothing to transition.
    }
}

impl Drop for WasdkComputePass {
    /// The engine's pass, dropped with this, ends the pass.
    fn drop(&mut self) {}
}

// Render bundles.

/// The encoder behind a [`wgpu::RenderBundleEncoder`] from this backend.
#[derive(Debug)]
pub struct WasdkRenderBundleEncoder {
    encoder: RenderBundleEncoder,
}

pub(crate) fn create_render_bundle_encoder(
    device: &Device,
    desc: &wgpu::RenderBundleEncoderDescriptor<'_>,
) -> DispatchRenderBundleEncoder {
    let descriptor = RenderBundleEncoderDescriptor {
        label: convert::label(desc.label),
        color_formats: (desc.color_formats.iter())
            .map(|format| format.map(convert::texture_format_to_wit))
            .collect(),
        depth_stencil: desc
            .depth_stencil
            .map(|depth_stencil| RenderBundleDepthStencil {
                format: convert::texture_format_to_wit(depth_stencil.format),
                depth_read_only: depth_stencil.depth_read_only,
                stencil_read_only: depth_stencil.stencil_read_only,
            }),
        sample_count: desc.sample_count,
        multiview: desc.multiview.map(std::num::NonZeroU32::get),
    };
    DispatchRenderBundleEncoder::custom(WasdkRenderBundleEncoder {
        encoder: device.create_render_bundle_encoder(&descriptor),
    })
}

impl RenderBundleEncoderInterface for WasdkRenderBundleEncoder {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        self.encoder
            .set_pipeline(WasdkRenderPipeline::of_dispatch(pipeline).wit_render_pipeline());
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        group: Option<&DispatchBindGroup>,
        offsets: &[DynamicOffset],
    ) {
        self.encoder
            .set_bind_group(index, bind_group(group), offsets);
    }

    fn set_index_buffer(
        &mut self,
        index_buffer: &DispatchBuffer,
        index_format: IndexFormat,
        offset: BufferAddress,
        size: Option<BufferSize>,
    ) {
        self.encoder.set_index_buffer(
            buffer(index_buffer),
            convert::index_format_to_wit(index_format),
            offset,
            size.map(BufferSize::get),
        );
    }

    fn set_vertex_buffer(
        &mut self,
        slot: u32,
        vertex_buffer: Option<&DispatchBuffer>,
        offset: BufferAddress,
        size: Option<BufferSize>,
    ) {
        self.encoder.set_vertex_buffer(
            slot,
            vertex_buffer.map(buffer),
            offset,
            size.map(BufferSize::get),
        );
    }

    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.encoder.set_immediates(offset, data);
    }

    fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        self.encoder.draw(
            convert::range_to_wit(vertices),
            convert::range_to_wit(instances),
        );
    }

    fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.encoder.draw_indexed(
            convert::range_to_wit(indices),
            base_vertex,
            convert::range_to_wit(instances),
        );
    }

    fn draw_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: BufferAddress) {
        self.encoder
            .draw_indirect(buffer(indirect_buffer), indirect_offset);
    }

    fn draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
    ) {
        self.encoder
            .draw_indexed_indirect(buffer(indirect_buffer), indirect_offset);
    }

    fn finish(self, desc: &wgpu::RenderBundleDescriptor<'_>) -> DispatchRenderBundle {
        DispatchRenderBundle::custom(WasdkRenderBundle {
            bundle: RenderBundleEncoder::finish(self.encoder, desc.label),
        })
    }

    fn finish_boxed(
        self: Box<Self>,
        desc: &wgpu::RenderBundleDescriptor<'_>,
    ) -> DispatchRenderBundle {
        (*self).finish(desc)
    }
}

/// The bundle behind a [`wgpu::RenderBundle`] from this backend.
#[derive(Debug)]
pub struct WasdkRenderBundle {
    bundle: RenderBundle,
}

downcast!(WasdkRenderBundle: dispatch DispatchRenderBundle);

impl WasdkRenderBundle {
    pub(crate) fn wit_render_bundle(&self) -> &RenderBundle {
        &self.bundle
    }
}

impl RenderBundleInterface for WasdkRenderBundle {}
