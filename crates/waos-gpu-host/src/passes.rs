//! Compute and render passes: each command one of wgpu-core's on the pass, its refusal reported
//! to the device; a pass ends when the program drops it, or its store goes with it.
//!
//! The invariant: a pass is ended in its `Drop` and nowhere else, so the encoder it records into
//! is unlocked whichever way the pass goes.

use std::sync::Arc;

use wasmtime::component::Resource;
use wgpu_core::command;
use wgpu_core::global::Global;

use crate::GpuCtx;
use crate::binding::BindGroup;
use crate::buffers::Buffer;
use crate::commands::QuerySet;
use crate::conversions::non_zero_u64;
use crate::errors::ErrorSink;
use crate::pipelines::{ComputePipeline, RenderPipeline};
use crate::render_bundles::RenderBundle;
use crate::wasdk::gpu::gpu::{self, Color, IndexFormat, Range};

/// A pass type over wgpu-core's pass `$core`, ended with `$end`, its end's error reported as
/// `$end_ident`: the pass, the instance it records on, and the device's errors.
macro_rules! pass {
    ($(#[$doc:meta])* $name:ident($core:ty), $end:ident, $end_ident:literal) => {
        $(#[$doc])*
        pub struct $name {
            pass: $core,
            instance: Arc<Global>,
            errors: Arc<ErrorSink>,
        }

        impl $name {
            pub(crate) fn new(pass: $core, instance: &Arc<Global>, errors: &Arc<ErrorSink>) -> $name {
                $name {
                    pass,
                    instance: Arc::clone(instance),
                    errors: Arc::clone(errors),
                }
            }

            /// Records a command with `record`, reporting its refusal under `fn_ident`.
            fn record<E>(
                &mut self,
                fn_ident: &'static str,
                record: impl FnOnce(&Global, &mut $core) -> Result<(), E>,
            ) where
                E: wgpu_types::error::WebGpuError + std::error::Error + Send + Sync + 'static,
            {
                let recorded = record(&self.instance, &mut self.pass);
                self.errors.report(recorded.err(), fn_ident, self.pass.label());
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                let ended = self.instance.$end(&mut self.pass);
                self.errors.report(ended.err(), $end_ident, self.pass.label());
            }
        }
    };
}

pass!(
    /// A compute pass as the program holds it.
    ComputePass(command::ComputePass),
    compute_pass_end,
    "ComputePass::end"
);
pass!(
    /// A render pass as the program holds it.
    RenderPass(command::RenderPass),
    render_pass_end,
    "RenderPass::end"
);

/// The pass a handle names, and the bind group a pass binds, looked up in the program's table.
impl GpuCtx<'_> {
    fn pass<P: 'static>(&mut self, pass: &Resource<P>) -> wasmtime::Result<&mut P> {
        Ok(self.table.get_mut(pass)?)
    }

    fn bind_group_id(
        &self,
        bind_group: &Option<Resource<BindGroup>>,
    ) -> wasmtime::Result<Option<wgpu_core::id::BindGroupId>> {
        bind_group
            .as_ref()
            .map(|bind_group| Ok(self.table.get(bind_group)?.id()))
            .transpose()
    }
}

impl gpu::HostComputePass for GpuCtx<'_> {
    fn set_pipeline(
        &mut self,
        pass: Resource<ComputePass>,
        pipeline: Resource<ComputePipeline>,
    ) -> wasmtime::Result<()> {
        let pipeline = self.table.get(&pipeline)?.id();
        self.pass(&pass)?
            .record("ComputePass::set_pipeline", |instance, pass| {
                instance.compute_pass_set_pipeline(pass, pipeline)
            });
        Ok(())
    }

    fn set_bind_group(
        &mut self,
        pass: Resource<ComputePass>,
        index: u32,
        bind_group: Option<Resource<BindGroup>>,
        offsets: Vec<u32>,
    ) -> wasmtime::Result<()> {
        let bind_group = self.bind_group_id(&bind_group)?;
        self.pass(&pass)?
            .record("ComputePass::set_bind_group", |instance, pass| {
                instance.compute_pass_set_bind_group(pass, index, bind_group, &offsets)
            });
        Ok(())
    }

    fn set_immediates(
        &mut self,
        pass: Resource<ComputePass>,
        offset: u32,
        data: Vec<u8>,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("ComputePass::set_immediates", |instance, pass| {
                instance.compute_pass_set_immediates(pass, offset, &data)
            });
        Ok(())
    }

    fn dispatch_workgroups(
        &mut self,
        pass: Resource<ComputePass>,
        x: u32,
        y: u32,
        z: u32,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("ComputePass::dispatch_workgroups", |instance, pass| {
                instance.compute_pass_dispatch_workgroups(pass, x, y, z)
            });
        Ok(())
    }

    fn dispatch_workgroups_indirect(
        &mut self,
        pass: Resource<ComputePass>,
        buffer: Resource<Buffer>,
        offset: u64,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.pass(&pass)?.record(
            "ComputePass::dispatch_workgroups_indirect",
            |instance, pass| {
                instance.compute_pass_dispatch_workgroups_indirect(pass, buffer, offset)
            },
        );
        Ok(())
    }

    fn write_timestamp(
        &mut self,
        pass: Resource<ComputePass>,
        query_set: Resource<QuerySet>,
        index: u32,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        self.pass(&pass)?
            .record("ComputePass::write_timestamp", |instance, pass| {
                instance.compute_pass_write_timestamp(pass, query_set, index)
            });
        Ok(())
    }

    fn begin_pipeline_statistics_query(
        &mut self,
        pass: Resource<ComputePass>,
        query_set: Resource<QuerySet>,
        index: u32,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        self.pass(&pass)?.record(
            "ComputePass::begin_pipeline_statistics_query",
            |instance, pass| {
                instance.compute_pass_begin_pipeline_statistics_query(pass, query_set, index)
            },
        );
        Ok(())
    }

    fn end_pipeline_statistics_query(
        &mut self,
        pass: Resource<ComputePass>,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?.record(
            "ComputePass::end_pipeline_statistics_query",
            |instance, pass| instance.compute_pass_end_pipeline_statistics_query(pass),
        );
        Ok(())
    }

    fn insert_debug_marker(
        &mut self,
        pass: Resource<ComputePass>,
        marker: String,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("ComputePass::insert_debug_marker", |instance, pass| {
                instance.compute_pass_insert_debug_marker(pass, &marker, 0)
            });
        Ok(())
    }

    fn push_debug_group(
        &mut self,
        pass: Resource<ComputePass>,
        group: String,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("ComputePass::push_debug_group", |instance, pass| {
                instance.compute_pass_push_debug_group(pass, &group, 0)
            });
        Ok(())
    }

    fn pop_debug_group(&mut self, pass: Resource<ComputePass>) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("ComputePass::pop_debug_group", |instance, pass| {
                instance.compute_pass_pop_debug_group(pass)
            });
        Ok(())
    }

    fn drop(&mut self, pass: Resource<ComputePass>) -> wasmtime::Result<()> {
        self.table.delete(pass)?;
        Ok(())
    }
}

impl gpu::HostRenderPass for GpuCtx<'_> {
    fn set_pipeline(
        &mut self,
        pass: Resource<RenderPass>,
        pipeline: Resource<RenderPipeline>,
    ) -> wasmtime::Result<()> {
        let pipeline = self.table.get(&pipeline)?.id();
        self.pass(&pass)?
            .record("RenderPass::set_pipeline", |instance, pass| {
                instance.render_pass_set_pipeline(pass, pipeline)
            });
        Ok(())
    }

    fn set_bind_group(
        &mut self,
        pass: Resource<RenderPass>,
        index: u32,
        bind_group: Option<Resource<BindGroup>>,
        offsets: Vec<u32>,
    ) -> wasmtime::Result<()> {
        let bind_group = self.bind_group_id(&bind_group)?;
        self.pass(&pass)?
            .record("RenderPass::set_bind_group", |instance, pass| {
                instance.render_pass_set_bind_group(pass, index, bind_group, &offsets)
            });
        Ok(())
    }

    fn set_index_buffer(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        format: IndexFormat,
        offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        let size = non_zero_u64(size, "set-index-buffer's size")?;
        self.pass(&pass)?
            .record("RenderPass::set_index_buffer", |instance, pass| {
                instance.render_pass_set_index_buffer(pass, buffer, format.into(), offset, size)
            });
        Ok(())
    }

    fn set_vertex_buffer(
        &mut self,
        pass: Resource<RenderPass>,
        slot: u32,
        buffer: Option<Resource<Buffer>>,
        offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<()> {
        let buffer = buffer
            .map(|buffer| Ok::<_, wasmtime::Error>(self.table.get(&buffer)?.id()))
            .transpose()?;
        let size = non_zero_u64(size, "set-vertex-buffer's size")?;
        self.pass(&pass)?
            .record("RenderPass::set_vertex_buffer", |instance, pass| {
                instance.render_pass_set_vertex_buffer(pass, slot, buffer, offset, size)
            });
        Ok(())
    }

    fn set_immediates(
        &mut self,
        pass: Resource<RenderPass>,
        offset: u32,
        data: Vec<u8>,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::set_immediates", |instance, pass| {
                instance.render_pass_set_immediates(pass, offset, &data)
            });
        Ok(())
    }

    fn set_blend_constant(
        &mut self,
        pass: Resource<RenderPass>,
        color: Color,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::set_blend_constant", |instance, pass| {
                instance.render_pass_set_blend_constant(pass, color.into())
            });
        Ok(())
    }

    fn set_scissor_rect(
        &mut self,
        pass: Resource<RenderPass>,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::set_scissor_rect", |instance, pass| {
                instance.render_pass_set_scissor_rect(pass, x, y, width, height)
            });
        Ok(())
    }

    fn set_viewport(
        &mut self,
        pass: Resource<RenderPass>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::set_viewport", |instance, pass| {
                instance.render_pass_set_viewport(pass, x, y, width, height, min_depth, max_depth)
            });
        Ok(())
    }

    fn set_stencil_reference(
        &mut self,
        pass: Resource<RenderPass>,
        reference: u32,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::set_stencil_reference", |instance, pass| {
                instance.render_pass_set_stencil_reference(pass, reference)
            });
        Ok(())
    }

    fn draw(
        &mut self,
        pass: Resource<RenderPass>,
        vertices: Range,
        instances: Range,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::draw", |instance, pass| {
                instance.render_pass_draw(
                    pass,
                    vertices.end.wrapping_sub(vertices.start),
                    instances.end.wrapping_sub(instances.start),
                    vertices.start,
                    instances.start,
                )
            });
        Ok(())
    }

    fn draw_indexed(
        &mut self,
        pass: Resource<RenderPass>,
        indices: Range,
        base_vertex: i32,
        instances: Range,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::draw_indexed", |instance, pass| {
                instance.render_pass_draw_indexed(
                    pass,
                    indices.end.wrapping_sub(indices.start),
                    instances.end.wrapping_sub(instances.start),
                    indices.start,
                    base_vertex,
                    instances.start,
                )
            });
        Ok(())
    }

    fn draw_indirect(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        offset: u64,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.pass(&pass)?
            .record("RenderPass::draw_indirect", |instance, pass| {
                instance.render_pass_draw_indirect(pass, buffer, offset)
            });
        Ok(())
    }

    fn draw_indexed_indirect(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        offset: u64,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.pass(&pass)?
            .record("RenderPass::draw_indexed_indirect", |instance, pass| {
                instance.render_pass_draw_indexed_indirect(pass, buffer, offset)
            });
        Ok(())
    }

    fn multi_draw_indirect(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        offset: u64,
        count: u32,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.pass(&pass)?
            .record("RenderPass::multi_draw_indirect", |instance, pass| {
                instance.render_pass_multi_draw_indirect(pass, buffer, offset, count)
            });
        Ok(())
    }

    fn multi_draw_indexed_indirect(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        offset: u64,
        count: u32,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.pass(&pass)?.record(
            "RenderPass::multi_draw_indexed_indirect",
            |instance, pass| {
                instance.render_pass_multi_draw_indexed_indirect(pass, buffer, offset, count)
            },
        );
        Ok(())
    }

    fn multi_draw_indirect_count(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        offset: u64,
        count_buffer: Resource<Buffer>,
        count_offset: u64,
        max_count: u32,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        let count_buffer = self.table.get(&count_buffer)?.id();
        self.pass(&pass)?
            .record("RenderPass::multi_draw_indirect_count", |instance, pass| {
                instance.render_pass_multi_draw_indirect_count(
                    pass,
                    buffer,
                    offset,
                    count_buffer,
                    count_offset,
                    max_count,
                )
            });
        Ok(())
    }

    fn multi_draw_indexed_indirect_count(
        &mut self,
        pass: Resource<RenderPass>,
        buffer: Resource<Buffer>,
        offset: u64,
        count_buffer: Resource<Buffer>,
        count_offset: u64,
        max_count: u32,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        let count_buffer = self.table.get(&count_buffer)?.id();
        self.pass(&pass)?.record(
            "RenderPass::multi_draw_indexed_indirect_count",
            |instance, pass| {
                instance.render_pass_multi_draw_indexed_indirect_count(
                    pass,
                    buffer,
                    offset,
                    count_buffer,
                    count_offset,
                    max_count,
                )
            },
        );
        Ok(())
    }

    fn execute_bundles(
        &mut self,
        pass: Resource<RenderPass>,
        bundles: Vec<Resource<RenderBundle>>,
    ) -> wasmtime::Result<()> {
        let bundles = bundles
            .iter()
            .map(|bundle| Ok(self.table.get(bundle)?.id()))
            .collect::<wasmtime::Result<Vec<_>>>()?;
        self.pass(&pass)?
            .record("RenderPass::execute_bundles", |instance, pass| {
                instance.render_pass_execute_bundles(pass, &bundles)
            });
        Ok(())
    }

    fn write_timestamp(
        &mut self,
        pass: Resource<RenderPass>,
        query_set: Resource<QuerySet>,
        index: u32,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        self.pass(&pass)?
            .record("RenderPass::write_timestamp", |instance, pass| {
                instance.render_pass_write_timestamp(pass, query_set, index)
            });
        Ok(())
    }

    fn begin_occlusion_query(
        &mut self,
        pass: Resource<RenderPass>,
        index: u32,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::begin_occlusion_query", |instance, pass| {
                instance.render_pass_begin_occlusion_query(pass, index)
            });
        Ok(())
    }

    fn end_occlusion_query(&mut self, pass: Resource<RenderPass>) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::end_occlusion_query", |instance, pass| {
                instance.render_pass_end_occlusion_query(pass)
            });
        Ok(())
    }

    fn begin_pipeline_statistics_query(
        &mut self,
        pass: Resource<RenderPass>,
        query_set: Resource<QuerySet>,
        index: u32,
    ) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        self.pass(&pass)?.record(
            "RenderPass::begin_pipeline_statistics_query",
            |instance, pass| {
                instance.render_pass_begin_pipeline_statistics_query(pass, query_set, index)
            },
        );
        Ok(())
    }

    fn end_pipeline_statistics_query(
        &mut self,
        pass: Resource<RenderPass>,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?.record(
            "RenderPass::end_pipeline_statistics_query",
            |instance, pass| instance.render_pass_end_pipeline_statistics_query(pass),
        );
        Ok(())
    }

    fn insert_debug_marker(
        &mut self,
        pass: Resource<RenderPass>,
        marker: String,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::insert_debug_marker", |instance, pass| {
                instance.render_pass_insert_debug_marker(pass, &marker, 0)
            });
        Ok(())
    }

    fn push_debug_group(
        &mut self,
        pass: Resource<RenderPass>,
        group: String,
    ) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::push_debug_group", |instance, pass| {
                instance.render_pass_push_debug_group(pass, &group, 0)
            });
        Ok(())
    }

    fn pop_debug_group(&mut self, pass: Resource<RenderPass>) -> wasmtime::Result<()> {
        self.pass(&pass)?
            .record("RenderPass::pop_debug_group", |instance, pass| {
                instance.render_pass_pop_debug_group(pass)
            });
        Ok(())
    }

    fn drop(&mut self, pass: Resource<RenderPass>) -> wasmtime::Result<()> {
        self.table.delete(pass)?;
        Ok(())
    }
}
