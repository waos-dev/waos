//! Bind group layouts and bind groups.

use wasdk::webgpu::{
    GpuBindGroup, GpuBindGroupDescriptor, GpuBindGroupEntry, GpuBindGroupLayout,
    GpuBindGroupLayoutDescriptor, GpuBindingResource, GpuBufferBinding, GpuDevice,
};
use wgpu::BindingResource;
use wgpu::custom::{
    BindGroupInterface, BindGroupLayoutInterface, DispatchBindGroup, DispatchBindGroupLayout,
};

use crate::buffer::WasdkBuffer;
use crate::convert;
use crate::gap::wasi_gap;
use crate::sampler::WasdkSampler;
use crate::texture::WasdkTextureView;

#[derive(Debug)]
pub struct WasdkBindGroupLayout {
    layout: GpuBindGroupLayout,
}

downcast!(WasdkBindGroupLayout: api wgpu::BindGroupLayout);

impl WasdkBindGroupLayout {
    pub(crate) fn new(layout: GpuBindGroupLayout) -> Self {
        WasdkBindGroupLayout { layout }
    }

    pub(crate) fn gpu_bind_group_layout(&self) -> &GpuBindGroupLayout {
        &self.layout
    }
}

impl BindGroupLayoutInterface for WasdkBindGroupLayout {}

pub(crate) fn create_bind_group_layout(
    device: &GpuDevice,
    desc: &wgpu::BindGroupLayoutDescriptor<'_>,
) -> DispatchBindGroupLayout {
    let descriptor = GpuBindGroupLayoutDescriptor {
        entries: desc
            .entries
            .iter()
            .map(convert::bind_group_layout_entry_to_wasi)
            .collect(),
        label: convert::label(desc.label),
    };
    DispatchBindGroupLayout::custom(WasdkBindGroupLayout::new(
        device.create_bind_group_layout(&descriptor),
    ))
}

#[derive(Debug)]
pub struct WasdkBindGroup {
    group: GpuBindGroup,
}

downcast!(WasdkBindGroup: dispatch DispatchBindGroup);

impl WasdkBindGroup {
    pub(crate) fn gpu_bind_group(&self) -> &GpuBindGroup {
        &self.group
    }
}

impl BindGroupInterface for WasdkBindGroup {}

pub(crate) fn create_bind_group(
    device: &GpuDevice,
    desc: &wgpu::BindGroupDescriptor<'_>,
) -> DispatchBindGroup {
    let descriptor = GpuBindGroupDescriptor {
        layout: WasdkBindGroupLayout::of(desc.layout).gpu_bind_group_layout(),
        entries: desc
            .entries
            .iter()
            .map(|entry| GpuBindGroupEntry {
                binding: entry.binding,
                resource: binding_resource_to_wasi(&entry.resource),
            })
            .collect(),
        label: convert::label(desc.label),
    };
    DispatchBindGroup::custom(WasdkBindGroup {
        group: device.create_bind_group(&descriptor),
    })
}

fn binding_resource_to_wasi<'a>(resource: &BindingResource<'a>) -> GpuBindingResource<'a> {
    match resource {
        BindingResource::Buffer(binding) => {
            GpuBindingResource::GpuBufferBinding(GpuBufferBinding {
                buffer: WasdkBuffer::of(binding.buffer).gpu_buffer(),
                offset: Some(binding.offset),
                size: binding.size.map(std::num::NonZeroU64::get),
            })
        }
        BindingResource::Sampler(sampler) => {
            GpuBindingResource::GpuSampler(WasdkSampler::of(sampler).gpu_sampler())
        }
        BindingResource::TextureView(view) => {
            GpuBindingResource::GpuTextureView(WasdkTextureView::of(view).gpu_texture_view())
        }
        BindingResource::BufferArray(_)
        | BindingResource::SamplerArray(_)
        | BindingResource::TextureViewArray(_) => {
            wasi_gap("binding arrays (BindingResource::*Array)")
        }
        BindingResource::AccelerationStructure(_)
        | BindingResource::AccelerationStructureArray(_) => {
            wasi_gap("ray tracing (BindingResource::AccelerationStructure)")
        }
        BindingResource::ExternalTexture(_) => {
            wasi_gap("external textures (BindingResource::ExternalTexture)")
        }
        // `BindingResource` is `#[non_exhaustive]`: a binding wgpu adds later.
        other => wasi_gap(format_args!("binding resource {other:?}")),
    }
}
