//! Sampler descriptors.

use wasdk::webgpu::{GpuAddressMode, GpuFilterMode, GpuMipmapFilterMode, GpuSamplerDescriptor};
use wgpu::{AddressMode, Features, FilterMode, MipmapFilterMode, SamplerDescriptor};

use super::{compare_function_to_wasi, label};
use crate::gap::feature_gap;

pub fn address_mode_to_wasi(mode: AddressMode) -> GpuAddressMode {
    match mode {
        AddressMode::ClampToEdge => GpuAddressMode::ClampToEdge,
        AddressMode::Repeat => GpuAddressMode::Repeat,
        AddressMode::MirrorRepeat => GpuAddressMode::MirrorRepeat,
        AddressMode::ClampToBorder => feature_gap(
            Features::ADDRESS_MODE_CLAMP_TO_BORDER,
            "AddressMode::ClampToBorder",
        ),
    }
}

pub fn filter_mode_to_wasi(mode: FilterMode) -> GpuFilterMode {
    match mode {
        FilterMode::Nearest => GpuFilterMode::Nearest,
        FilterMode::Linear => GpuFilterMode::Linear,
    }
}

pub fn mipmap_filter_mode_to_wasi(mode: MipmapFilterMode) -> GpuMipmapFilterMode {
    match mode {
        MipmapFilterMode::Nearest => GpuMipmapFilterMode::Nearest,
        MipmapFilterMode::Linear => GpuMipmapFilterMode::Linear,
    }
}

pub fn sampler_descriptor_to_wasi(desc: &SamplerDescriptor<'_>) -> GpuSamplerDescriptor {
    if desc.border_color.is_some() {
        feature_gap(
            Features::ADDRESS_MODE_CLAMP_TO_BORDER,
            "SamplerDescriptor::border_color",
        );
    }
    GpuSamplerDescriptor {
        address_mode_u: Some(address_mode_to_wasi(desc.address_mode_u)),
        address_mode_v: Some(address_mode_to_wasi(desc.address_mode_v)),
        address_mode_w: Some(address_mode_to_wasi(desc.address_mode_w)),
        mag_filter: Some(filter_mode_to_wasi(desc.mag_filter)),
        min_filter: Some(filter_mode_to_wasi(desc.min_filter)),
        mipmap_filter: Some(mipmap_filter_mode_to_wasi(desc.mipmap_filter)),
        lod_min_clamp: Some(desc.lod_min_clamp),
        lod_max_clamp: Some(desc.lod_max_clamp),
        compare: desc.compare.map(compare_function_to_wasi),
        max_anisotropy: Some(desc.anisotropy_clamp),
        label: label(desc.label),
    }
}
