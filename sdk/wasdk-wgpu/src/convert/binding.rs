//! Bind group layouts: what each binding takes.

use wasdk::gpu_types as wit;

use super::{
    enum_conversions, label, shader_stages_to_wit, texture_format_to_wit,
    texture_view_dimension_to_wit,
};

enum_conversions! {
    sampler_binding_type_to_wit, sampler_binding_type_from_wit:
        SamplerBindingType <=> SamplerBindingType {
        Filtering, NonFiltering, Comparison,
    }
}

enum_conversions! {
    storage_texture_access_to_wit, storage_texture_access_from_wit:
        StorageTextureAccess <=> StorageTextureAccess {
        WriteOnly, ReadOnly, ReadWrite, Atomic,
    }
}

/// wgpu's `Storage { read_only }` is WebGPU's two cases, as the WIT names them.
pub fn buffer_binding_type_to_wit(ty: wgpu::BufferBindingType) -> wit::BufferBindingType {
    match ty {
        wgpu::BufferBindingType::Uniform => wit::BufferBindingType::Uniform,
        wgpu::BufferBindingType::Storage { read_only: false } => wit::BufferBindingType::Storage,
        wgpu::BufferBindingType::Storage { read_only: true } => {
            wit::BufferBindingType::ReadOnlyStorage
        }
    }
}

/// wgpu's `Float { filterable }` is WebGPU's two cases, as the WIT names them.
pub fn texture_sample_type_to_wit(ty: wgpu::TextureSampleType) -> wit::TextureSampleType {
    match ty {
        wgpu::TextureSampleType::Float { filterable: true } => wit::TextureSampleType::Float,
        wgpu::TextureSampleType::Float { filterable: false } => {
            wit::TextureSampleType::UnfilterableFloat
        }
        wgpu::TextureSampleType::Depth => wit::TextureSampleType::Depth,
        wgpu::TextureSampleType::Sint => wit::TextureSampleType::Sint,
        wgpu::TextureSampleType::Uint => wit::TextureSampleType::Uint,
    }
}

pub fn binding_type_to_wit(ty: &wgpu::BindingType) -> wit::BindingType {
    match *ty {
        wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset,
            min_binding_size,
        } => wit::BindingType::Buffer(wit::BufferBindingLayout {
            ty: buffer_binding_type_to_wit(ty),
            has_dynamic_offset,
            min_binding_size: min_binding_size.map(wgpu::BufferSize::get),
        }),
        wgpu::BindingType::Sampler(ty) => {
            wit::BindingType::Sampler(sampler_binding_type_to_wit(ty))
        }
        wgpu::BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled,
        } => wit::BindingType::Texture(wit::TextureBindingLayout {
            sample_type: texture_sample_type_to_wit(sample_type),
            view_dimension: texture_view_dimension_to_wit(view_dimension),
            multisampled,
        }),
        wgpu::BindingType::StorageTexture {
            access,
            format,
            view_dimension,
        } => wit::BindingType::StorageTexture(wit::StorageTextureBindingLayout {
            access: storage_texture_access_to_wit(access),
            format: texture_format_to_wit(format),
            view_dimension: texture_view_dimension_to_wit(view_dimension),
        }),
        wgpu::BindingType::ExternalTexture => wit::BindingType::ExternalTexture,
        wgpu::BindingType::AccelerationStructure { .. } => {
            crate::left_out::left_out("ray tracing (BindingType::AccelerationStructure)")
        }
    }
}

pub fn bind_group_layout_descriptor_to_wit(
    desc: &wgpu::BindGroupLayoutDescriptor<'_>,
) -> wit::BindGroupLayoutDescriptor {
    wit::BindGroupLayoutDescriptor {
        label: label(desc.label),
        entries: (desc.entries.iter())
            .map(|entry| wit::BindGroupLayoutEntry {
                binding: entry.binding,
                visibility: shader_stages_to_wit(entry.visibility),
                ty: binding_type_to_wit(&entry.ty),
                count: entry.count.map(std::num::NonZeroU32::get),
            })
            .collect(),
    }
}
