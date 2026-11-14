//! A surface's configuration: how its picture blends and the colours it holds, as a view's canvas
//! context is configured with them.

use wasdk::webgpu::{
    GpuCanvasAlphaMode, GpuCanvasToneMapping, GpuCanvasToneMappingMode, PredefinedColorSpace,
};
use wgpu::{CompositeAlphaMode, SurfaceColorSpace};

use crate::gap::wasi_gap;

/// How the canvas blends its picture: `Auto` is WebGPU's default, opaque, as wgpu's browser
/// backend takes it.
pub fn alpha_mode_to_wasi(mode: CompositeAlphaMode) -> GpuCanvasAlphaMode {
    match mode {
        CompositeAlphaMode::Auto | CompositeAlphaMode::Opaque => GpuCanvasAlphaMode::Opaque,
        CompositeAlphaMode::PreMultiplied => GpuCanvasAlphaMode::Premultiplied,
        CompositeAlphaMode::PostMultiplied | CompositeAlphaMode::Inherit => {
            wasi_gap(format_args!("the surface alpha mode {mode:?}"))
        }
    }
}

/// The canvas's colour space and tone mapping for `space`: `Auto` and `Srgb` leave WebGPU's
/// defaults, an sRGB canvas with standard tone mapping; an extended space is its colour space
/// with extended tone mapping, as wgpu's browser backend sets it.
pub fn color_space_to_wasi(
    space: SurfaceColorSpace,
) -> (Option<PredefinedColorSpace>, Option<GpuCanvasToneMapping>) {
    let extended = || {
        Some(GpuCanvasToneMapping {
            mode: Some(GpuCanvasToneMappingMode::Extended),
        })
    };
    match space {
        SurfaceColorSpace::Auto | SurfaceColorSpace::Srgb => (None, None),
        SurfaceColorSpace::DisplayP3 => (Some(PredefinedColorSpace::DisplayP3), None),
        SurfaceColorSpace::ExtendedSrgb => (Some(PredefinedColorSpace::Srgb), extended()),
        SurfaceColorSpace::ExtendedDisplayP3 => (Some(PredefinedColorSpace::DisplayP3), extended()),
        other => wasi_gap(format_args!("the surface colour space {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_alpha_mode_is_opaque_as_webgpus() {
        assert!(matches!(
            alpha_mode_to_wasi(CompositeAlphaMode::Auto),
            GpuCanvasAlphaMode::Opaque
        ));
        assert!(matches!(
            alpha_mode_to_wasi(CompositeAlphaMode::PreMultiplied),
            GpuCanvasAlphaMode::Premultiplied
        ));
    }

    #[test]
    #[should_panic(expected = "PostMultiplied")]
    fn an_alpha_mode_webgpu_has_no_name_for_is_a_gap() {
        alpha_mode_to_wasi(CompositeAlphaMode::PostMultiplied);
    }

    #[test]
    fn the_default_colour_space_leaves_webgpus_defaults() {
        let (space, tone_mapping) = color_space_to_wasi(SurfaceColorSpace::Auto);
        assert!(space.is_none() && tone_mapping.is_none());
    }

    #[test]
    fn an_extended_colour_space_is_its_space_with_extended_tone_mapping() {
        let (space, tone_mapping) = color_space_to_wasi(SurfaceColorSpace::ExtendedDisplayP3);
        assert!(matches!(space, Some(PredefinedColorSpace::DisplayP3)));
        assert!(matches!(
            tone_mapping,
            Some(GpuCanvasToneMapping {
                mode: Some(GpuCanvasToneMappingMode::Extended)
            })
        ));
    }

    #[test]
    #[should_panic(expected = "Bt2100Pq")]
    fn a_colour_space_webgpu_has_no_name_for_is_a_gap() {
        color_space_to_wasi(SurfaceColorSpace::Bt2100Pq);
    }
}
