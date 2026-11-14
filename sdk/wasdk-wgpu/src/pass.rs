//! Render passes, compute passes, and render bundles: what draws are recorded into.

use wasdk::webgpu::{
    GpuBindGroup, GpuBuffer, GpuCommandEncoder, GpuComputePassDescriptor, GpuComputePassEncoder,
    GpuComputePassTimestampWrites, GpuDevice, GpuRenderBundle, GpuRenderBundleDescriptor,
    GpuRenderBundleEncoder, GpuRenderBundleEncoderDescriptor, GpuRenderPassColorAttachment,
    GpuRenderPassDepthStencilAttachment, GpuRenderPassDescriptor, GpuRenderPassEncoder,
    GpuRenderPassTimestampWrites, SetBindGroupError,
};
use wgpu::custom::{
    ComputePassInterface, DispatchBindGroup, DispatchBuffer, DispatchComputePass,
    DispatchComputePipeline, DispatchQuerySet, DispatchRenderBundle, DispatchRenderBundleEncoder,
    DispatchRenderPass, DispatchRenderPipeline, DispatchTextureView, RenderBundleEncoderInterface,
    RenderBundleInterface, RenderPassInterface,
};
use wgpu::{BufferAddress, BufferSize, Color, DynamicOffset, Features, IndexFormat};

use crate::binding::WasdkBindGroup;
use crate::buffer::WasdkBuffer;
use crate::convert;
use crate::gap::{feature_gap, wasi_gap};
use crate::pipeline::{WasdkComputePipeline, WasdkRenderPipeline};
use crate::query::WasdkQuerySet;
use crate::texture::WasdkTextureView;

/// The size of one `DrawIndirectArgs`, the stride `multi_draw_indirect` unrolls by.
const DRAW_INDIRECT_STRIDE: BufferAddress = 16;
/// The size of one `DrawIndexedIndirectArgs`.
const DRAW_INDEXED_INDIRECT_STRIDE: BufferAddress = 20;

fn bind_group(group: Option<&DispatchBindGroup>) -> Option<&GpuBindGroup> {
    group.map(|group| WasdkBindGroup::of_dispatch(group).gpu_bind_group())
}

/// wgpu passes an empty offset list where WebGPU passes none.
fn dynamic_offsets(offsets: &[DynamicOffset]) -> Option<&[DynamicOffset]> {
    (!offsets.is_empty()).then_some(offsets)
}

fn bind_group_set(result: Result<(), SetBindGroupError>) {
    result.unwrap_or_else(|error| panic!("wasdk-wgpu: set_bind_group: {}", error.message));
}

fn buffer(buffer: &DispatchBuffer) -> &GpuBuffer {
    WasdkBuffer::of_dispatch(buffer).gpu_buffer()
}

// Render passes.

#[derive(Debug)]
pub struct WasdkRenderPass {
    pass: GpuRenderPassEncoder,
}

pub(crate) fn begin_render_pass(
    encoder: &GpuCommandEncoder,
    desc: &wgpu::RenderPassDescriptor<'_>,
) -> DispatchRenderPass {
    if desc.multiview_mask.is_some() {
        feature_gap(Features::MULTIVIEW, "RenderPassDescriptor::multiview_mask");
    }
    let descriptor = GpuRenderPassDescriptor {
        color_attachments: desc
            .color_attachments
            .iter()
            .map(|attachment| attachment.as_ref().map(color_attachment_to_wasi))
            .collect(),
        depth_stencil_attachment: desc
            .depth_stencil_attachment
            .as_ref()
            .map(depth_stencil_attachment_to_wasi),
        occlusion_query_set: desc
            .occlusion_query_set
            .map(|query_set| WasdkQuerySet::of(query_set).gpu_query_set()),
        timestamp_writes: desc.timestamp_writes.as_ref().map(|writes| {
            GpuRenderPassTimestampWrites {
                query_set: WasdkQuerySet::of(writes.query_set).gpu_query_set(),
                beginning_of_pass_write_index: writes.beginning_of_pass_write_index,
                end_of_pass_write_index: writes.end_of_pass_write_index,
            }
        }),
        max_draw_count: None,
        label: convert::label(desc.label),
    };
    DispatchRenderPass::custom(WasdkRenderPass {
        pass: encoder.begin_render_pass(&descriptor),
    })
}

fn color_attachment_to_wasi<'a>(
    attachment: &wgpu::RenderPassColorAttachment<'a>,
) -> GpuRenderPassColorAttachment<'a> {
    let ops = convert::operations_to_wasi(&attachment.ops, Color::BLACK);
    GpuRenderPassColorAttachment {
        view: WasdkTextureView::of(attachment.view).gpu_texture_view(),
        depth_slice: attachment.depth_slice,
        resolve_target: attachment
            .resolve_target
            .map(|view| WasdkTextureView::of(view).gpu_texture_view()),
        clear_value: ops.clear_value.map(convert::color_to_wasi),
        load_op: ops.load,
        store_op: ops.store,
    }
}

/// An aspect without operations is read-only for the pass, as wgpu defines it.
fn depth_stencil_attachment_to_wasi<'a>(
    attachment: &wgpu::RenderPassDepthStencilAttachment<'a>,
) -> GpuRenderPassDepthStencilAttachment<'a> {
    let depth = attachment
        .depth_ops
        .as_ref()
        .map(|ops| convert::operations_to_wasi(ops, 1.0));
    let stencil = attachment
        .stencil_ops
        .as_ref()
        .map(|ops| convert::operations_to_wasi(ops, 0));
    GpuRenderPassDepthStencilAttachment {
        view: WasdkTextureView::of(attachment.view).gpu_texture_view(),
        depth_clear_value: depth.and_then(|ops| ops.clear_value),
        depth_load_op: depth.map(|ops| ops.load),
        depth_store_op: depth.map(|ops| ops.store),
        depth_read_only: Some(depth.is_none()),
        stencil_clear_value: stencil.and_then(|ops| ops.clear_value),
        stencil_load_op: stencil.map(|ops| ops.load),
        stencil_store_op: stencil.map(|ops| ops.store),
        stencil_read_only: Some(stencil.is_none()),
    }
}

impl RenderPassInterface for WasdkRenderPass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        self.pass
            .set_pipeline(WasdkRenderPipeline::of_dispatch(pipeline).gpu_render_pipeline());
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        group: Option<&DispatchBindGroup>,
        offsets: &[DynamicOffset],
    ) {
        bind_group_set(self.pass.set_bind_group(
            index,
            bind_group(group),
            dynamic_offsets(offsets),
            None,
            None,
        ));
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
            convert::index_format_to_wasi(index_format),
            Some(offset),
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
            Some(offset),
            size.map(BufferSize::get),
        );
    }

    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.pass.set_immediates(offset, data, None, None);
    }

    fn set_blend_constant(&mut self, color: Color) {
        self.pass.set_blend_constant(convert::color_to_wasi(color));
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
            vertices.end - vertices.start,
            Some(instances.end - instances.start),
            Some(vertices.start),
            Some(instances.start),
        );
    }

    fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.pass.draw_indexed(
            indices.end - indices.start,
            Some(instances.end - instances.start),
            Some(indices.start),
            Some(base_vertex),
            Some(instances.start),
        );
    }

    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        wasi_gap("mesh shaders (RenderPass::draw_mesh_tasks)")
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
        wasi_gap("mesh shaders (RenderPass::draw_mesh_tasks_indirect)")
    }

    fn multi_draw_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
        count: u32,
    ) {
        let indirect_buffer = buffer(indirect_buffer);
        for draw in 0..BufferAddress::from(count) {
            self.pass.draw_indirect(
                indirect_buffer,
                indirect_offset + draw * DRAW_INDIRECT_STRIDE,
            );
        }
    }

    fn multi_draw_indexed_indirect(
        &mut self,
        indirect_buffer: &DispatchBuffer,
        indirect_offset: BufferAddress,
        count: u32,
    ) {
        let indirect_buffer = buffer(indirect_buffer);
        for draw in 0..BufferAddress::from(count) {
            self.pass.draw_indexed_indirect(
                indirect_buffer,
                indirect_offset + draw * DRAW_INDEXED_INDIRECT_STRIDE,
            );
        }
    }

    fn multi_draw_mesh_tasks_indirect(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
        _count: u32,
    ) {
        wasi_gap("mesh shaders (RenderPass::multi_draw_mesh_tasks_indirect)")
    }

    fn multi_draw_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: BufferAddress,
        _max_count: u32,
    ) {
        feature_gap(
            Features::MULTI_DRAW_INDIRECT_COUNT,
            "RenderPass::multi_draw_indirect_count",
        )
    }

    fn multi_draw_indexed_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: BufferAddress,
        _max_count: u32,
    ) {
        feature_gap(
            Features::MULTI_DRAW_INDIRECT_COUNT,
            "RenderPass::multi_draw_indexed_indirect_count",
        )
    }

    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: BufferAddress,
        _max_count: u32,
    ) {
        wasi_gap("mesh shaders (RenderPass::multi_draw_mesh_tasks_indirect_count)")
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

    fn write_timestamp(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        wasi_gap(
            "timestamps inside a pass (RenderPass::write_timestamp); WebGPU writes them at pass boundaries",
        )
    }

    fn begin_occlusion_query(&mut self, query_index: u32) {
        self.pass.begin_occlusion_query(query_index);
    }

    fn end_occlusion_query(&mut self) {
        self.pass.end_occlusion_query();
    }

    fn begin_pipeline_statistics_query(
        &mut self,
        _query_set: &DispatchQuerySet,
        _query_index: u32,
    ) {
        feature_gap(
            Features::PIPELINE_STATISTICS_QUERY,
            "RenderPass::begin_pipeline_statistics_query",
        )
    }

    fn end_pipeline_statistics_query(&mut self) {
        feature_gap(
            Features::PIPELINE_STATISTICS_QUERY,
            "RenderPass::end_pipeline_statistics_query",
        )
    }

    fn execute_bundles(&mut self, render_bundles: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        let bundles: Vec<&GpuRenderBundle> = render_bundles
            .map(|bundle| WasdkRenderBundle::of_dispatch(bundle).gpu_render_bundle())
            .collect();
        self.pass.execute_bundles(&bundles);
    }
}

impl Drop for WasdkRenderPass {
    fn drop(&mut self) {
        self.pass.end();
    }
}

// Compute passes.

#[derive(Debug)]
pub struct WasdkComputePass {
    pass: GpuComputePassEncoder,
}

pub(crate) fn begin_compute_pass(
    encoder: &GpuCommandEncoder,
    desc: &wgpu::ComputePassDescriptor<'_>,
) -> DispatchComputePass {
    let descriptor = GpuComputePassDescriptor {
        timestamp_writes: desc.timestamp_writes.as_ref().map(|writes| {
            GpuComputePassTimestampWrites {
                query_set: WasdkQuerySet::of(writes.query_set).gpu_query_set(),
                beginning_of_pass_write_index: writes.beginning_of_pass_write_index,
                end_of_pass_write_index: writes.end_of_pass_write_index,
            }
        }),
        label: convert::label(desc.label),
    };
    DispatchComputePass::custom(WasdkComputePass {
        pass: encoder.begin_compute_pass(Some(&descriptor)),
    })
}

impl ComputePassInterface for WasdkComputePass {
    fn set_pipeline(&mut self, pipeline: &DispatchComputePipeline) {
        self.pass
            .set_pipeline(WasdkComputePipeline::of_dispatch(pipeline).gpu_compute_pipeline());
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        group: Option<&DispatchBindGroup>,
        offsets: &[DynamicOffset],
    ) {
        bind_group_set(self.pass.set_bind_group(
            index,
            bind_group(group),
            dynamic_offsets(offsets),
            None,
            None,
        ));
    }

    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.pass.set_immediates(offset, data, None, None);
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

    fn write_timestamp(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        wasi_gap(
            "timestamps inside a pass (ComputePass::write_timestamp); WebGPU writes them at pass boundaries",
        )
    }

    fn begin_pipeline_statistics_query(
        &mut self,
        _query_set: &DispatchQuerySet,
        _query_index: u32,
    ) {
        feature_gap(
            Features::PIPELINE_STATISTICS_QUERY,
            "ComputePass::begin_pipeline_statistics_query",
        )
    }

    fn end_pipeline_statistics_query(&mut self) {
        feature_gap(
            Features::PIPELINE_STATISTICS_QUERY,
            "ComputePass::end_pipeline_statistics_query",
        )
    }

    fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        self.pass.dispatch_workgroups(x, Some(y), Some(z));
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
        // WebGPU tracks resource state itself; there is nothing to transition.
    }
}

impl Drop for WasdkComputePass {
    fn drop(&mut self) {
        self.pass.end();
    }
}

// Render bundles.

#[derive(Debug)]
pub struct WasdkRenderBundleEncoder {
    encoder: GpuRenderBundleEncoder,
}

pub(crate) fn create_render_bundle_encoder(
    device: &GpuDevice,
    desc: &wgpu::RenderBundleEncoderDescriptor<'_>,
) -> DispatchRenderBundleEncoder {
    if desc.multiview.is_some() {
        feature_gap(
            Features::MULTIVIEW,
            "RenderBundleEncoderDescriptor::multiview",
        );
    }
    let descriptor = GpuRenderBundleEncoderDescriptor {
        depth_read_only: desc.depth_stencil.map(|ds| ds.depth_read_only),
        stencil_read_only: desc.depth_stencil.map(|ds| ds.stencil_read_only),
        color_formats: desc
            .color_formats
            .iter()
            .map(|format| format.map(convert::texture_format_to_wasi))
            .collect(),
        depth_stencil_format: desc
            .depth_stencil
            .map(|ds| convert::texture_format_to_wasi(ds.format)),
        sample_count: Some(desc.sample_count),
        label: convert::label(desc.label),
    };
    DispatchRenderBundleEncoder::custom(WasdkRenderBundleEncoder {
        encoder: device.create_render_bundle_encoder(&descriptor),
    })
}

impl RenderBundleEncoderInterface for WasdkRenderBundleEncoder {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        self.encoder
            .set_pipeline(WasdkRenderPipeline::of_dispatch(pipeline).gpu_render_pipeline());
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        group: Option<&DispatchBindGroup>,
        offsets: &[DynamicOffset],
    ) {
        bind_group_set(self.encoder.set_bind_group(
            index,
            bind_group(group),
            dynamic_offsets(offsets),
            None,
            None,
        ));
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
            convert::index_format_to_wasi(index_format),
            Some(offset),
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
            Some(offset),
            size.map(BufferSize::get),
        );
    }

    fn set_immediates(&mut self, offset: u32, data: &[u8]) {
        self.encoder.set_immediates(offset, data, None, None);
    }

    fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        self.encoder.draw(
            vertices.end - vertices.start,
            Some(instances.end - instances.start),
            Some(vertices.start),
            Some(instances.start),
        );
    }

    fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.encoder.draw_indexed(
            indices.end - indices.start,
            Some(instances.end - instances.start),
            Some(indices.start),
            Some(base_vertex),
            Some(instances.start),
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
        let descriptor = GpuRenderBundleDescriptor {
            label: convert::label(desc.label),
        };
        DispatchRenderBundle::custom(WasdkRenderBundle {
            bundle: self.encoder.finish(Some(&descriptor)),
        })
    }

    fn finish_boxed(
        self: Box<Self>,
        desc: &wgpu::RenderBundleDescriptor<'_>,
    ) -> DispatchRenderBundle {
        (*self).finish(desc)
    }
}

#[derive(Debug)]
pub struct WasdkRenderBundle {
    bundle: GpuRenderBundle,
}

downcast!(WasdkRenderBundle: dispatch DispatchRenderBundle);

impl WasdkRenderBundle {
    pub(crate) fn gpu_render_bundle(&self) -> &GpuRenderBundle {
        &self.bundle
    }
}

impl RenderBundleInterface for WasdkRenderBundle {}
