//! A surface's vocabulary: present and alpha modes, colour spaces, what a surface can be
//! configured with, its configuration, and what its display says of its range.

use wasdk::gpu_types as wit;

use super::{
    enum_conversions, texture_format_from_wit, texture_format_to_wit, texture_usages_from_wit,
    texture_usages_to_wit,
};

enum_conversions! {
    present_mode_to_wit, present_mode_from_wit: PresentMode <=> PresentMode {
        AutoVsync, AutoNoVsync, Fifo, FifoRelaxed, Immediate, Mailbox,
    }
}

enum_conversions! {
    alpha_mode_to_wit, alpha_mode_from_wit: CompositeAlphaMode <=> CompositeAlphaMode {
        Auto, Opaque, PreMultiplied, PostMultiplied, Inherit,
    }
}

enum_conversions! {
    color_space_to_wit, color_space_from_wit: SurfaceColorSpace <=> SurfaceColorSpace {
        Auto, Srgb, ExtendedSrgbLinear, DisplayP3, Bt2100Pq, Bt2100Hlg, ExtendedSrgb,
        ExtendedDisplayP3,
    }
}

/// The display's gamut; only this direction, since wgpu's `DisplayGamut` is `#[non_exhaustive]`.
fn display_gamut_from_wit(gamut: wit::DisplayGamut) -> wgpu::DisplayGamut {
    match gamut {
        wit::DisplayGamut::Srgb => wgpu::DisplayGamut::Srgb,
        wit::DisplayGamut::DisplayP3 => wgpu::DisplayGamut::DisplayP3,
        wit::DisplayGamut::Rec2020 => wgpu::DisplayGamut::Rec2020,
    }
}

/// The colour spaces the WIT lists as wgpu's set of them; `Auto`, which is no space of its own,
/// adds none.
fn color_spaces_from_wit(spaces: Vec<wit::SurfaceColorSpace>) -> wgpu::SurfaceColorSpaces {
    (spaces.into_iter())
        .filter_map(|space| color_space_from_wit(space).to_color_spaces())
        .fold(wgpu::SurfaceColorSpaces::empty(), |all, space| all | space)
}

pub fn surface_capabilities_from_wit(
    capabilities: wit::SurfaceCapabilities,
) -> wgpu::SurfaceCapabilities {
    wgpu::SurfaceCapabilities {
        formats: (capabilities.formats.into_iter())
            .map(texture_format_from_wit)
            .collect(),
        format_capabilities: (capabilities.format_capabilities.into_iter())
            .map(|format| wgpu::SurfaceFormatCapabilities {
                format: texture_format_from_wit(format.format),
                color_spaces: color_spaces_from_wit(format.color_spaces),
            })
            .collect(),
        present_modes: (capabilities.present_modes.into_iter())
            .map(present_mode_from_wit)
            .collect(),
        alpha_modes: (capabilities.alpha_modes.into_iter())
            .map(alpha_mode_from_wit)
            .collect(),
        usages: texture_usages_from_wit(capabilities.usages),
    }
}

pub fn surface_configuration_to_wit(
    config: &wgpu::SurfaceConfiguration,
) -> wit::SurfaceConfiguration {
    wit::SurfaceConfiguration {
        usage: texture_usages_to_wit(config.usage),
        format: texture_format_to_wit(config.format),
        color_space: color_space_to_wit(config.color_space),
        width: config.width,
        height: config.height,
        present_mode: present_mode_to_wit(config.present_mode),
        desired_maximum_frame_latency: config.desired_maximum_frame_latency,
        alpha_mode: alpha_mode_to_wit(config.alpha_mode),
        view_formats: (config.view_formats.iter().copied())
            .map(texture_format_to_wit)
            .collect(),
    }
}

pub fn display_hdr_info_from_wit(info: wit::DisplayHdrInfo) -> wgpu::DisplayHdrInfo {
    let point = |point: Option<(f32, f32)>| point.map(|(x, y)| [x, y]);
    wgpu::DisplayHdrInfo {
        luminance: info.luminance.map(|luminance| wgpu::DisplayLuminance {
            max_nits: luminance.max_nits,
            max_full_frame_nits: luminance.max_full_frame_nits,
            min_nits: luminance.min_nits,
            sdr_white_nits: luminance.sdr_white_nits,
        }),
        headroom: info.headroom.map(|headroom| wgpu::DisplayHeadroom {
            current: headroom.current,
            potential: headroom.potential,
            reference: headroom.reference,
        }),
        chromaticity: info
            .chromaticity
            .map(|chromaticity| wgpu::DisplayChromaticity {
                red: point(chromaticity.red),
                green: point(chromaticity.green),
                blue: point(chromaticity.blue),
                white: point(chromaticity.white),
            }),
        coarse: info.coarse.map(|coarse| wgpu::DisplayCoarseRange {
            high_dynamic_range: coarse.high_dynamic_range,
            gamut: coarse.gamut.map(display_gamut_from_wit),
        }),
        bits_per_color: info.bits_per_color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_colour_spaces_are_wgpus_set_without_auto() {
        let spaces = color_spaces_from_wit(vec![
            wit::SurfaceColorSpace::Auto,
            wit::SurfaceColorSpace::Srgb,
            wit::SurfaceColorSpace::Bt2100Pq,
        ]);
        assert_eq!(
            spaces,
            wgpu::SurfaceColorSpaces::SRGB | wgpu::SurfaceColorSpaces::BT2100_PQ
        );
    }
}
