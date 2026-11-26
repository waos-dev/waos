//! Pipeline layouts and render and compute pipelines.

use wasdk::gpu::{
    ComputePipeline, ComputePipelineDescriptor, Device, FragmentState, PipelineLayout,
    PipelineLayoutDescriptor, RenderPipeline, RenderPipelineDescriptor, VertexState,
};
use wgpu::custom::{
    ComputePipelineInterface, DispatchBindGroupLayout, DispatchComputePipeline,
    DispatchPipelineLayout, DispatchRenderPipeline, PipelineLayoutInterface,
    RenderPipelineInterface,
};

use crate::binding::WasdkBindGroupLayout;
use crate::convert;
use crate::left_out::left_out;
use crate::shader::WasdkShaderModule;

/// The layout behind a [`wgpu::PipelineLayout`] from this backend.
#[derive(Debug)]
pub struct WasdkPipelineLayout {
    layout: PipelineLayout,
}

downcast!(WasdkPipelineLayout: api wgpu::PipelineLayout);

impl PipelineLayoutInterface for WasdkPipelineLayout {}

pub(crate) fn create_pipeline_layout(
    device: &Device,
    desc: &wgpu::PipelineLayoutDescriptor<'_>,
) -> DispatchPipelineLayout {
    let descriptor = PipelineLayoutDescriptor {
        label: convert::label(desc.label),
        bind_group_layouts: (desc.bind_group_layouts.iter())
            .map(|layout| {
                layout.map(|layout| WasdkBindGroupLayout::of(layout).wit_bind_group_layout())
            })
            .collect(),
        immediate_size: desc.immediate_size,
    };
    DispatchPipelineLayout::custom(WasdkPipelineLayout {
        layout: device.create_pipeline_layout(&descriptor),
    })
}

/// The layout a pipeline descriptor names; none for one the pipeline makes from its shaders.
fn layout_of(layout: Option<&wgpu::PipelineLayout>) -> Option<&PipelineLayout> {
    layout.map(|layout| &WasdkPipelineLayout::of(layout).layout)
}

/// A pipeline cache is the engine's to keep, for every program's pipelines alike.
fn check_no_cache(cache: Option<&wgpu::PipelineCache>) {
    if cache.is_some() {
        left_out("pipeline caches (a pipeline descriptor's `cache`), which the engine keeps");
    }
}

/// The pipeline behind a [`wgpu::RenderPipeline`] from this backend.
#[derive(Debug)]
pub struct WasdkRenderPipeline {
    pipeline: RenderPipeline,
}

downcast!(WasdkRenderPipeline: dispatch DispatchRenderPipeline);

impl WasdkRenderPipeline {
    pub(crate) fn wit_render_pipeline(&self) -> &RenderPipeline {
        &self.pipeline
    }
}

impl RenderPipelineInterface for WasdkRenderPipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(WasdkBindGroupLayout::new(
            self.pipeline.bind_group_layout(index),
        ))
    }
}

pub(crate) fn create_render_pipeline(
    device: &Device,
    desc: &wgpu::RenderPipelineDescriptor<'_>,
) -> DispatchRenderPipeline {
    check_no_cache(desc.cache);
    let vertex = VertexState {
        module: WasdkShaderModule::of(desc.vertex.module).wit_shader_module(),
        entry_point: desc.vertex.entry_point.map(str::to_owned),
        compilation_options: convert::pipeline_compilation_options_to_wit(
            &desc.vertex.compilation_options,
        ),
        buffers: (desc.vertex.buffers.iter())
            .map(|layout| layout.as_ref().map(convert::vertex_buffer_layout_to_wit))
            .collect(),
    };
    let fragment = desc.fragment.as_ref().map(|fragment| FragmentState {
        module: WasdkShaderModule::of(fragment.module).wit_shader_module(),
        entry_point: fragment.entry_point.map(str::to_owned),
        compilation_options: convert::pipeline_compilation_options_to_wit(
            &fragment.compilation_options,
        ),
        targets: (fragment.targets.iter())
            .map(|target| target.as_ref().map(convert::color_target_to_wit))
            .collect(),
    });
    let descriptor = RenderPipelineDescriptor {
        label: convert::label(desc.label),
        layout: layout_of(desc.layout),
        vertex,
        primitive: convert::primitive_state_to_wit(&desc.primitive),
        depth_stencil: (desc.depth_stencil.as_ref()).map(convert::depth_stencil_state_to_wit),
        multisample: convert::multisample_state_to_wit(&desc.multisample),
        fragment,
        multiview_mask: desc.multiview_mask.map(std::num::NonZeroU32::get),
    };
    DispatchRenderPipeline::custom(WasdkRenderPipeline {
        pipeline: device.create_render_pipeline(&descriptor),
    })
}

/// The pipeline behind a [`wgpu::ComputePipeline`] from this backend.
#[derive(Debug)]
pub struct WasdkComputePipeline {
    pipeline: ComputePipeline,
}

downcast!(WasdkComputePipeline: dispatch DispatchComputePipeline);

impl WasdkComputePipeline {
    pub(crate) fn wit_compute_pipeline(&self) -> &ComputePipeline {
        &self.pipeline
    }
}

impl ComputePipelineInterface for WasdkComputePipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(WasdkBindGroupLayout::new(
            self.pipeline.bind_group_layout(index),
        ))
    }
}

pub(crate) fn create_compute_pipeline(
    device: &Device,
    desc: &wgpu::ComputePipelineDescriptor<'_>,
) -> DispatchComputePipeline {
    check_no_cache(desc.cache);
    let descriptor = ComputePipelineDescriptor {
        label: convert::label(desc.label),
        layout: layout_of(desc.layout),
        module: WasdkShaderModule::of(desc.module).wit_shader_module(),
        entry_point: desc.entry_point.map(str::to_owned),
        compilation_options: convert::pipeline_compilation_options_to_wit(
            &desc.compilation_options,
        ),
    };
    DispatchComputePipeline::custom(WasdkComputePipeline {
        pipeline: device.create_compute_pipeline(&descriptor),
    })
}
