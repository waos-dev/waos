//! Bind group layouts and bind groups.

use wasdk::gpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource,
    BufferBinding, Device,
};
use wgpu::custom::{
    BindGroupInterface, BindGroupLayoutInterface, DispatchBindGroup, DispatchBindGroupLayout,
};

use crate::buffer::WasdkBuffer;
use crate::convert;
use crate::left_out::left_out;
use crate::sampler::WasdkSampler;
use crate::texture::{WasdkExternalTexture, WasdkTextureView};

/// The layout behind a [`wgpu::BindGroupLayout`] from this backend.
#[derive(Debug)]
pub struct WasdkBindGroupLayout {
    layout: BindGroupLayout,
}

downcast!(WasdkBindGroupLayout: api wgpu::BindGroupLayout);

impl WasdkBindGroupLayout {
    pub(crate) fn new(layout: BindGroupLayout) -> Self {
        WasdkBindGroupLayout { layout }
    }

    pub(crate) fn wit_bind_group_layout(&self) -> &BindGroupLayout {
        &self.layout
    }
}

impl BindGroupLayoutInterface for WasdkBindGroupLayout {}

pub(crate) fn create_bind_group_layout(
    device: &Device,
    desc: &wgpu::BindGroupLayoutDescriptor<'_>,
) -> DispatchBindGroupLayout {
    let descriptor = convert::bind_group_layout_descriptor_to_wit(desc);
    DispatchBindGroupLayout::custom(WasdkBindGroupLayout::new(
        device.create_bind_group_layout(&descriptor),
    ))
}

/// The bind group behind a [`wgpu::BindGroup`] from this backend.
#[derive(Debug)]
pub struct WasdkBindGroup {
    group: BindGroup,
}

downcast!(WasdkBindGroup: dispatch DispatchBindGroup);

impl WasdkBindGroup {
    pub(crate) fn wit_bind_group(&self) -> &BindGroup {
        &self.group
    }
}

impl BindGroupInterface for WasdkBindGroup {}

pub(crate) fn create_bind_group(
    device: &Device,
    desc: &wgpu::BindGroupDescriptor<'_>,
) -> DispatchBindGroup {
    let descriptor = BindGroupDescriptor {
        label: convert::label(desc.label),
        layout: WasdkBindGroupLayout::of(desc.layout).wit_bind_group_layout(),
        entries: (desc.entries.iter())
            .map(|entry| BindGroupEntry {
                binding: entry.binding,
                resource: binding_resource_to_wit(&entry.resource),
            })
            .collect(),
    };
    DispatchBindGroup::custom(WasdkBindGroup {
        group: device.create_bind_group(&descriptor),
    })
}

fn buffer_binding_to_wit<'a>(binding: &wgpu::BufferBinding<'a>) -> BufferBinding<'a> {
    BufferBinding {
        buffer: WasdkBuffer::of(binding.buffer).wit_buffer(),
        offset: binding.offset,
        size: binding.size.map(wgpu::BufferSize::get),
    }
}

fn binding_resource_to_wit<'a>(resource: &wgpu::BindingResource<'a>) -> BindingResource<'a> {
    match resource {
        wgpu::BindingResource::Buffer(binding) => {
            BindingResource::Buffer(buffer_binding_to_wit(binding))
        }
        wgpu::BindingResource::BufferArray(bindings) => {
            BindingResource::BufferArray(bindings.iter().map(buffer_binding_to_wit).collect())
        }
        wgpu::BindingResource::Sampler(sampler) => {
            BindingResource::Sampler(WasdkSampler::of(sampler).wit_sampler())
        }
        wgpu::BindingResource::SamplerArray(samplers) => BindingResource::SamplerArray(
            (samplers.iter())
                .map(|sampler| WasdkSampler::of(sampler).wit_sampler())
                .collect(),
        ),
        wgpu::BindingResource::TextureView(view) => {
            BindingResource::TextureView(WasdkTextureView::of(view).wit_texture_view())
        }
        wgpu::BindingResource::TextureViewArray(views) => BindingResource::TextureViewArray(
            (views.iter())
                .map(|view| WasdkTextureView::of(view).wit_texture_view())
                .collect(),
        ),
        wgpu::BindingResource::ExternalTexture(texture) => BindingResource::ExternalTexture(
            WasdkExternalTexture::of(texture).wit_external_texture(),
        ),
        wgpu::BindingResource::AccelerationStructure(_)
        | wgpu::BindingResource::AccelerationStructureArray(_) => {
            left_out("ray tracing (BindingResource::AccelerationStructure)")
        }
        // `BindingResource` is `#[non_exhaustive]`: a binding wgpu adds later.
        other => left_out(format_args!("the binding resource {other:?}")),
    }
}
