//! Render and compute pipelines, and the bind group layouts a pipeline answers. A stage's
//! workgroup memory is always zeroed, as WebGPU zeroes it: memory left uninitialized could hold
//! another program's data.

use std::borrow::Cow;
use std::sync::Arc;

use wasmtime::component::{Resource, ResourceTable};
use wgpu_core::id::{ComputePipelineId, RenderPipelineId, ShaderModuleId};
use wgpu_core::pipeline as core;

use crate::binding::BindGroupLayout;
use crate::conversions::{label, non_zero_u32};
use crate::devices::Device;
use crate::errors::ErrorSink;
use crate::shaders::ShaderModule;
use crate::wasdk::gpu::gpu::{
    self, ComputePipelineDescriptor, PipelineCompilationOptions, RenderPipelineDescriptor,
};
use crate::{GpuCtx, GpuObject};

/// A render pipeline, with its device's errors, which `bind-group-layout` reports to.
pub struct RenderPipeline {
    object: GpuObject<RenderPipelineId>,
    errors: Arc<ErrorSink>,
}

/// A compute pipeline, as [`RenderPipeline`] is.
pub struct ComputePipeline {
    object: GpuObject<ComputePipelineId>,
    errors: Arc<ErrorSink>,
}

impl RenderPipeline {
    pub(crate) fn id(&self) -> RenderPipelineId {
        self.object.id()
    }
}

impl ComputePipeline {
    pub(crate) fn id(&self) -> ComputePipelineId {
        self.object.id()
    }
}

/// A stage of `module`'s entry point, compiled with `options`.
fn stage(
    table: &ResourceTable,
    module: &Resource<ShaderModule>,
    entry_point: Option<String>,
    options: PipelineCompilationOptions,
) -> wasmtime::Result<core::ProgrammableStageDescriptor<'static, ShaderModuleId>> {
    Ok(core::ProgrammableStageDescriptor {
        module: table.get(module)?.id(),
        entry_point: entry_point.map(Cow::Owned),
        constants: options.constants.into_iter().collect(),
        zero_initialize_workgroup_memory: true,
    })
}

/// The render pipeline `descriptor` describes, on `device`.
pub(crate) fn create_render_pipeline(
    ctx: &GpuCtx<'_>,
    device: &Resource<Device>,
    descriptor: RenderPipelineDescriptor,
) -> wasmtime::Result<RenderPipeline> {
    let table = &*ctx.table;
    let vertex = descriptor.vertex;
    let buffers = vertex
        .buffers
        .into_iter()
        .map(|layout| layout.map(Into::into))
        .collect::<Vec<_>>();
    let fragment = descriptor
        .fragment
        .map(|fragment| -> wasmtime::Result<_> {
            Ok(core::FragmentState {
                stage: stage(
                    table,
                    &fragment.module,
                    fragment.entry_point,
                    fragment.compilation_options,
                )?,
                targets: Cow::Owned(
                    fragment
                        .targets
                        .into_iter()
                        .map(|target| target.map(Into::into))
                        .collect(),
                ),
            })
        })
        .transpose()?;
    let core_descriptor = core::RenderPipelineDescriptor {
        label: label(descriptor.label),
        layout: descriptor
            .layout
            .map(|layout| Ok::<_, wasmtime::Error>(table.get(&layout)?.id()))
            .transpose()?,
        vertex: core::VertexState {
            stage: stage(
                table,
                &vertex.module,
                vertex.entry_point,
                vertex.compilation_options,
            )?,
            buffers: Cow::Owned(buffers),
        },
        primitive: descriptor.primitive.into(),
        depth_stencil: descriptor.depth_stencil.map(Into::into),
        multisample: descriptor.multisample.into(),
        fragment,
        multiview_mask: non_zero_u32(
            descriptor.multiview_mask,
            "a render pipeline's multiview mask",
        )?,
        cache: None,
    };
    let device = table.get(device)?;
    let (pipeline, error) =
        ctx.instance
            .device_create_render_pipeline(device.device_id(), &core_descriptor, None);
    device.errors.report(
        error,
        "Device::create_render_pipeline",
        core_descriptor.label.as_deref(),
    );
    Ok(RenderPipeline {
        object: GpuObject::new(pipeline, ctx.instance),
        errors: Arc::clone(&device.errors),
    })
}

/// The compute pipeline `descriptor` describes, on `device`.
pub(crate) fn create_compute_pipeline(
    ctx: &GpuCtx<'_>,
    device: &Resource<Device>,
    descriptor: ComputePipelineDescriptor,
) -> wasmtime::Result<ComputePipeline> {
    let table = &*ctx.table;
    let core_descriptor = core::ComputePipelineDescriptor {
        label: label(descriptor.label),
        layout: descriptor
            .layout
            .map(|layout| Ok::<_, wasmtime::Error>(table.get(&layout)?.id()))
            .transpose()?,
        stage: stage(
            table,
            &descriptor.module,
            descriptor.entry_point,
            descriptor.compilation_options,
        )?,
        cache: None,
    };
    let device = table.get(device)?;
    let (pipeline, error) =
        ctx.instance
            .device_create_compute_pipeline(device.device_id(), &core_descriptor, None);
    device.errors.report(
        error,
        "Device::create_compute_pipeline",
        core_descriptor.label.as_deref(),
    );
    Ok(ComputePipeline {
        object: GpuObject::new(pipeline, ctx.instance),
        errors: Arc::clone(&device.errors),
    })
}

impl gpu::HostRenderPipeline for GpuCtx<'_> {
    fn bind_group_layout(
        &mut self,
        pipeline: Resource<RenderPipeline>,
        index: u32,
    ) -> wasmtime::Result<Resource<BindGroupLayout>> {
        let pipeline = self.table.get(&pipeline)?;
        let (layout, error) =
            self.instance
                .render_pipeline_get_bind_group_layout(pipeline.id(), index, None);
        pipeline
            .errors
            .report(error, "RenderPipeline::get_bind_group_layout", None);
        Ok(self.table.push(GpuObject::new(layout, self.instance))?)
    }

    fn drop(&mut self, pipeline: Resource<RenderPipeline>) -> wasmtime::Result<()> {
        self.table.delete(pipeline)?;
        Ok(())
    }
}

impl gpu::HostComputePipeline for GpuCtx<'_> {
    fn bind_group_layout(
        &mut self,
        pipeline: Resource<ComputePipeline>,
        index: u32,
    ) -> wasmtime::Result<Resource<BindGroupLayout>> {
        let pipeline = self.table.get(&pipeline)?;
        let (layout, error) =
            self.instance
                .compute_pipeline_get_bind_group_layout(pipeline.id(), index, None);
        pipeline
            .errors
            .report(error, "ComputePipeline::get_bind_group_layout", None);
        Ok(self.table.push(GpuObject::new(layout, self.instance))?)
    }

    fn drop(&mut self, pipeline: Resource<ComputePipeline>) -> wasmtime::Result<()> {
        self.table.delete(pipeline)?;
        Ok(())
    }
}
