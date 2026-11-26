//! Sampler descriptors.

use wasdk::gpu_types as wit;

use super::{compare_function_to_wit, enum_conversions, label};

enum_conversions! {
    address_mode_to_wit, address_mode_from_wit: AddressMode <=> AddressMode {
        ClampToEdge, Repeat, MirrorRepeat, ClampToBorder,
    }
}

enum_conversions! {
    filter_mode_to_wit, filter_mode_from_wit: FilterMode <=> FilterMode {
        Nearest, Linear,
    }
}

enum_conversions! {
    mipmap_filter_mode_to_wit, mipmap_filter_mode_from_wit: MipmapFilterMode <=> MipmapFilterMode {
        Nearest, Linear,
    }
}

enum_conversions! {
    sampler_border_color_to_wit, sampler_border_color_from_wit:
        SamplerBorderColor <=> SamplerBorderColor {
        TransparentBlack, OpaqueBlack, OpaqueWhite, Zero,
    }
}

pub fn sampler_descriptor_to_wit(desc: &wgpu::SamplerDescriptor<'_>) -> wit::SamplerDescriptor {
    wit::SamplerDescriptor {
        label: label(desc.label),
        address_mode_u: address_mode_to_wit(desc.address_mode_u),
        address_mode_v: address_mode_to_wit(desc.address_mode_v),
        address_mode_w: address_mode_to_wit(desc.address_mode_w),
        mag_filter: filter_mode_to_wit(desc.mag_filter),
        min_filter: filter_mode_to_wit(desc.min_filter),
        mipmap_filter: mipmap_filter_mode_to_wit(desc.mipmap_filter),
        lod_min_clamp: desc.lod_min_clamp,
        lod_max_clamp: desc.lod_max_clamp,
        compare: desc.compare.map(compare_function_to_wit),
        anisotropy_clamp: desc.anisotropy_clamp,
        border_color: desc.border_color.map(sampler_border_color_to_wit),
    }
}
