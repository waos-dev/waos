//! Pipeline layouts and render and compute pipelines.

use wasdk::webgpu::{
    GpuComputePipeline, GpuComputePipelineDescriptor, GpuDevice, GpuFragmentState, GpuLayoutMode,
    GpuPipelineLayout, GpuPipelineLayoutDescriptor, GpuProgrammableStage, GpuRenderPipeline,
    GpuRenderPipelineDescriptor, GpuVertexState, RecordGpuPipelineConstantValue,
};
use wgpu::custom::{
    ComputePipelineInterface, DispatchBindGroupLayout, DispatchComputePipeline,
    DispatchPipelineLayout, DispatchRenderPipeline, PipelineLayoutInterface,
    RenderPipelineInterface,
};

use crate::binding::WasdkBindGroupLayout;
use crate::convert;
use crate::gap::{feature_gap, wasi_gap};
use crate::shader::WasdkShaderModule;

#[derive(Debug)]
pub struct WasdkPipelineLayout {
    layout: GpuPipelineLayout,
}

downcast!(WasdkPipelineLayout: api wgpu::PipelineLayout);

impl PipelineLayoutInterface for WasdkPipelineLayout {}

pub(crate) fn create_pipeline_layout(
    device: &GpuDevice,
    desc: &wgpu::PipelineLayoutDescriptor<'_>,
) -> DispatchPipelineLayout {
    let descriptor = GpuPipelineLayoutDescriptor {
        bind_group_layouts: desc
            .bind_group_layouts
            .iter()
            .map(|layout| {
                layout.map(|layout| WasdkBindGroupLayout::of(layout).gpu_bind_group_layout())
            })
            .collect(),
        immediate_size: Some(desc.immediate_size),
        label: convert::label(desc.label),
    };
    DispatchPipelineLayout::custom(WasdkPipelineLayout {
        layout: device.create_pipeline_layout(&descriptor),
    })
}

#[derive(Debug)]
pub struct WasdkRenderPipeline {
    pipeline: GpuRenderPipeline,
}

downcast!(WasdkRenderPipeline: dispatch DispatchRenderPipeline);

impl WasdkRenderPipeline {
    pub(crate) fn gpu_render_pipeline(&self) -> &GpuRenderPipeline {
        &self.pipeline
    }
}

impl RenderPipelineInterface for WasdkRenderPipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(WasdkBindGroupLayout::new(
            self.pipeline.get_bind_group_layout(index),
        ))
    }
}

pub(crate) fn create_render_pipeline(
    device: &GpuDevice,
    desc: &wgpu::RenderPipelineDescriptor<'_>,
) -> DispatchRenderPipeline {
    if desc.cache.is_some() {
        wasi_gap("pipeline caches (RenderPipelineDescriptor::cache)");
    }
    if desc.multiview_mask.is_some() {
        feature_gap(
            wgpu::Features::MULTIVIEW,
            "RenderPipelineDescriptor::multiview_mask",
        );
    }
    let vertex = GpuVertexState {
        buffers: Some(
            desc.vertex
                .buffers
                .iter()
                .map(|layout| layout.as_ref().map(convert::vertex_buffer_layout_to_wasi))
                .collect(),
        ),
        module: WasdkShaderModule::of(desc.vertex.module).gpu_shader_module(),
        entry_point: desc.vertex.entry_point.map(str::to_owned),
        constants: constants_to_wasi(&desc.vertex.compilation_options),
    };
    let fragment = desc.fragment.as_ref().map(|fragment| GpuFragmentState {
        targets: fragment
            .targets
            .iter()
            .map(|target| target.as_ref().map(convert::color_target_to_wasi))
            .collect(),
        module: WasdkShaderModule::of(fragment.module).gpu_shader_module(),
        entry_point: fragment.entry_point.map(str::to_owned),
        constants: constants_to_wasi(&fragment.compilation_options),
    });
    let descriptor = GpuRenderPipelineDescriptor {
        vertex,
        primitive: Some(convert::primitive_state_to_wasi(&desc.primitive)),
        depth_stencil: desc
            .depth_stencil
            .as_ref()
            .map(convert::depth_stencil_state_to_wasi),
        multisample: Some(convert::multisample_state_to_wasi(&desc.multisample)),
        fragment,
        layout: layout_mode(desc.layout),
        label: convert::label(desc.label),
    };
    DispatchRenderPipeline::custom(WasdkRenderPipeline {
        pipeline: device.create_render_pipeline(descriptor),
    })
}

#[derive(Debug)]
pub struct WasdkComputePipeline {
    pipeline: GpuComputePipeline,
}

downcast!(WasdkComputePipeline: dispatch DispatchComputePipeline);

impl WasdkComputePipeline {
    pub(crate) fn gpu_compute_pipeline(&self) -> &GpuComputePipeline {
        &self.pipeline
    }
}

impl ComputePipelineInterface for WasdkComputePipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(WasdkBindGroupLayout::new(
            self.pipeline.get_bind_group_layout(index),
        ))
    }
}

pub(crate) fn create_compute_pipeline(
    device: &GpuDevice,
    desc: &wgpu::ComputePipelineDescriptor<'_>,
) -> DispatchComputePipeline {
    if desc.cache.is_some() {
        wasi_gap("pipeline caches (ComputePipelineDescriptor::cache)");
    }
    let descriptor = GpuComputePipelineDescriptor {
        compute: GpuProgrammableStage {
            module: WasdkShaderModule::of(desc.module).gpu_shader_module(),
            entry_point: desc.entry_point.map(str::to_owned),
            constants: constants_to_wasi(&desc.compilation_options),
        },
        layout: layout_mode(desc.layout),
        label: convert::label(desc.label),
    };
    DispatchComputePipeline::custom(WasdkComputePipeline {
        pipeline: device.create_compute_pipeline(descriptor),
    })
}

fn layout_mode(layout: Option<&wgpu::PipelineLayout>) -> GpuLayoutMode<'_> {
    match layout {
        Some(layout) => GpuLayoutMode::Specific(&WasdkPipelineLayout::of(layout).layout),
        None => GpuLayoutMode::Auto,
    }
}

/// The pipeline's override constants. `zero_initialize_workgroup_memory` has no counterpart:
/// WebGPU always zeroes workgroup memory.
fn constants_to_wasi(
    options: &wgpu::PipelineCompilationOptions<'_>,
) -> Option<RecordGpuPipelineConstantValue> {
    if options.constants.is_empty() {
        return None;
    }
    let record = RecordGpuPipelineConstantValue::new();
    for (key, value) in options.constants {
        record.add(key, *value);
    }
    Some(record)
}
