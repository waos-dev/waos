//! What binds resources to pipelines: bind group layouts, bind groups over the program's
//! buffers, samplers and views, and pipeline layouts.

use std::borrow::Cow;

use wasmtime::component::{Resource, ResourceTable};
use wgpu_core::binding_model as core;
use wgpu_core::id::{BindGroupId, BindGroupLayoutId, PipelineLayoutId};

use crate::conversions::label;
use crate::devices::Device;
use crate::wasdk::gpu::gpu::{
    self, BindGroupDescriptor, BindingResource, BufferBinding, PipelineLayoutDescriptor,
};
use crate::{GpuCtx, GpuObject};

pub type BindGroupLayout = GpuObject<BindGroupLayoutId>;
pub type BindGroup = GpuObject<BindGroupId>;
pub type PipelineLayout = GpuObject<PipelineLayoutId>;

/// The bind group `descriptor` describes, on `device`.
pub(crate) fn create_bind_group(
    ctx: &GpuCtx<'_>,
    device: &Resource<Device>,
    descriptor: BindGroupDescriptor,
) -> wasmtime::Result<BindGroup> {
    let table = &*ctx.table;
    let entries = descriptor
        .entries
        .into_iter()
        .map(|entry| {
            Ok(core::BindGroupEntry {
                binding: entry.binding,
                resource: binding_resource(table, entry.resource)?,
            })
        })
        .collect::<wasmtime::Result<Vec<_>>>()?;
    let core_descriptor = core::BindGroupDescriptor {
        label: label(descriptor.label),
        layout: table.get(&descriptor.layout)?.id(),
        entries: Cow::Owned(entries),
    };
    let device = table.get(device)?;
    let (bind_group, error) =
        ctx.instance
            .device_create_bind_group(device.device_id(), &core_descriptor, None);
    device.errors.report(
        error,
        "Device::create_bind_group",
        core_descriptor.label.as_deref(),
    );
    Ok(GpuObject::new(bind_group, ctx.instance))
}

fn binding_resource(
    table: &ResourceTable,
    resource: BindingResource,
) -> wasmtime::Result<core::BindingResource<'static>> {
    let views = |views: Vec<Resource<gpu::TextureView>>| -> wasmtime::Result<Vec<_>> {
        views.iter().map(|view| Ok(table.get(view)?.id())).collect()
    };
    Ok(match resource {
        BindingResource::Buffer(binding) => {
            core::BindingResource::Buffer(buffer_binding(table, binding)?)
        }
        BindingResource::BufferArray(bindings) => core::BindingResource::BufferArray(Cow::Owned(
            bindings
                .into_iter()
                .map(|binding| buffer_binding(table, binding))
                .collect::<wasmtime::Result<_>>()?,
        )),
        BindingResource::Sampler(sampler) => {
            core::BindingResource::Sampler(table.get(&sampler)?.id())
        }
        BindingResource::SamplerArray(samplers) => core::BindingResource::SamplerArray(Cow::Owned(
            samplers
                .iter()
                .map(|sampler| Ok(table.get(sampler)?.id()))
                .collect::<wasmtime::Result<_>>()?,
        )),
        BindingResource::TextureView(view) => {
            core::BindingResource::TextureView(table.get(&view)?.id())
        }
        BindingResource::TextureViewArray(array) => {
            core::BindingResource::TextureViewArray(Cow::Owned(views(array)?))
        }
        BindingResource::ExternalTexture(texture) => {
            core::BindingResource::ExternalTexture(table.get(&texture)?.id())
        }
    })
}

fn buffer_binding(
    table: &ResourceTable,
    binding: BufferBinding,
) -> wasmtime::Result<core::BufferBinding> {
    Ok(core::BufferBinding {
        buffer: table.get(&binding.buffer)?.id(),
        offset: binding.offset,
        size: binding.size,
    })
}

/// The pipeline layout `descriptor` describes, on `device`.
pub(crate) fn create_pipeline_layout(
    ctx: &GpuCtx<'_>,
    device: &Resource<Device>,
    descriptor: PipelineLayoutDescriptor,
) -> wasmtime::Result<PipelineLayout> {
    let bind_group_layouts = descriptor
        .bind_group_layouts
        .iter()
        .map(|layout| {
            layout
                .as_ref()
                .map(|layout| Ok(ctx.table.get(layout)?.id()))
                .transpose()
        })
        .collect::<wasmtime::Result<Vec<_>>>()?;
    let core_descriptor = core::PipelineLayoutDescriptor {
        label: label(descriptor.label),
        bind_group_layouts: Cow::Owned(bind_group_layouts),
        immediate_size: descriptor.immediate_size,
    };
    let device = ctx.table.get(device)?;
    let (layout, error) =
        ctx.instance
            .device_create_pipeline_layout(device.device_id(), &core_descriptor, None);
    device.errors.report(
        error,
        "Device::create_pipeline_layout",
        core_descriptor.label.as_deref(),
    );
    Ok(GpuObject::new(layout, ctx.instance))
}

impl gpu::HostBindGroupLayout for GpuCtx<'_> {
    fn drop(&mut self, layout: Resource<BindGroupLayout>) -> wasmtime::Result<()> {
        self.table.delete(layout)?;
        Ok(())
    }
}

impl gpu::HostBindGroup for GpuCtx<'_> {
    fn drop(&mut self, bind_group: Resource<BindGroup>) -> wasmtime::Result<()> {
        self.table.delete(bind_group)?;
        Ok(())
    }
}

impl gpu::HostPipelineLayout for GpuCtx<'_> {
    fn drop(&mut self, layout: Resource<PipelineLayout>) -> wasmtime::Result<()> {
        self.table.delete(layout)?;
        Ok(())
    }
}
