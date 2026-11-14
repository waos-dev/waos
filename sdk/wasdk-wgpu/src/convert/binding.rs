//! Bind group layout entries.

use wasdk::webgpu::{
    GpuBindGroupLayoutEntry, GpuBufferBindingLayout, GpuBufferBindingType, GpuSamplerBindingLayout,
    GpuSamplerBindingType, GpuStorageTextureAccess, GpuStorageTextureBindingLayout,
    GpuTextureBindingLayout, GpuTextureSampleType,
};
use wgpu::{
    BindGroupLayoutEntry, BindingType, BufferBindingType, Features, SamplerBindingType,
    StorageTextureAccess, TextureSampleType,
};

use super::{shader_stages_to_wasi, texture_format_to_wasi, texture_view_dimension_to_wasi};
use crate::gap::{feature_gap, wasi_gap};

pub fn buffer_binding_type_to_wasi(ty: BufferBindingType) -> GpuBufferBindingType {
    match ty {
        BufferBindingType::Uniform => GpuBufferBindingType::Uniform,
        BufferBindingType::Storage { read_only: false } => GpuBufferBindingType::Storage,
        BufferBindingType::Storage { read_only: true } => GpuBufferBindingType::ReadOnlyStorage,
    }
}

pub fn sampler_binding_type_to_wasi(ty: SamplerBindingType) -> GpuSamplerBindingType {
    match ty {
        SamplerBindingType::Filtering => GpuSamplerBindingType::Filtering,
        SamplerBindingType::NonFiltering => GpuSamplerBindingType::NonFiltering,
        SamplerBindingType::Comparison => GpuSamplerBindingType::Comparison,
    }
}

pub fn texture_sample_type_to_wasi(ty: TextureSampleType) -> GpuTextureSampleType {
    match ty {
        TextureSampleType::Float { filterable: true } => GpuTextureSampleType::Float,
        TextureSampleType::Float { filterable: false } => GpuTextureSampleType::UnfilterableFloat,
        TextureSampleType::Depth => GpuTextureSampleType::Depth,
        TextureSampleType::Sint => GpuTextureSampleType::Sint,
        TextureSampleType::Uint => GpuTextureSampleType::Uint,
    }
}

pub fn storage_texture_access_to_wasi(access: StorageTextureAccess) -> GpuStorageTextureAccess {
    match access {
        StorageTextureAccess::WriteOnly => GpuStorageTextureAccess::WriteOnly,
        StorageTextureAccess::ReadOnly => GpuStorageTextureAccess::ReadOnly,
        StorageTextureAccess::ReadWrite => GpuStorageTextureAccess::ReadWrite,
        StorageTextureAccess::Atomic => {
            feature_gap(Features::TEXTURE_ATOMIC, "StorageTextureAccess::Atomic")
        }
    }
}

pub fn bind_group_layout_entry_to_wasi(entry: &BindGroupLayoutEntry) -> GpuBindGroupLayoutEntry {
    if entry.count.is_some() {
        wasi_gap("binding arrays (BindGroupLayoutEntry::count)");
    }
    let mut wasi = GpuBindGroupLayoutEntry {
        binding: entry.binding,
        visibility: shader_stages_to_wasi(entry.visibility),
        buffer: None,
        sampler: None,
        texture: None,
        storage_texture: None,
    };
    match entry.ty {
        BindingType::Buffer {
            ty,
            has_dynamic_offset,
            min_binding_size,
        } => {
            wasi.buffer = Some(GpuBufferBindingLayout {
                type_: Some(buffer_binding_type_to_wasi(ty)),
                has_dynamic_offset: Some(has_dynamic_offset),
                min_binding_size: min_binding_size.map(std::num::NonZeroU64::get),
            });
        }
        BindingType::Sampler(ty) => {
            wasi.sampler = Some(GpuSamplerBindingLayout {
                type_: Some(sampler_binding_type_to_wasi(ty)),
            });
        }
        BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled,
        } => {
            wasi.texture = Some(GpuTextureBindingLayout {
                sample_type: Some(texture_sample_type_to_wasi(sample_type)),
                view_dimension: Some(texture_view_dimension_to_wasi(view_dimension)),
                multisampled: Some(multisampled),
            });
        }
        BindingType::StorageTexture {
            access,
            format,
            view_dimension,
        } => {
            wasi.storage_texture = Some(GpuStorageTextureBindingLayout {
                access: Some(storage_texture_access_to_wasi(access)),
                format: texture_format_to_wasi(format),
                view_dimension: Some(texture_view_dimension_to_wasi(view_dimension)),
            });
        }
        BindingType::AccelerationStructure { .. } => {
            wasi_gap("ray tracing (BindingType::AccelerationStructure)")
        }
        BindingType::ExternalTexture => {
            wasi_gap("external textures (BindingType::ExternalTexture)")
        }
    }
    wasi
}
