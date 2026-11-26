//! The enums: each case named as the other crate names it, converted by exhaustive matches; both
//! ways where a value crosses back to the program, one way where it only goes to wgpu-core.

use wgpu_types as wgt;

use super::rows;
use crate::wasdk::gpu::types;

/// `From` both ways between `types::$wit` and `wgt::$wgt`.
macro_rules! both_ways {
    ($wit:ident <=> $wgt:ident { $($rows:tt)* }) => {
        rows!(both_ways_impl! ($wit $wgt) [] $($rows)*);
    };
}

macro_rules! both_ways_impl {
    (($wit:ident $wgt:ident) [$(($a:ident $b:ident))*]) => {
        impl From<types::$wit> for wgt::$wgt {
            fn from(value: types::$wit) -> Self {
                match value {
                    $(types::$wit::$a => wgt::$wgt::$b,)*
                }
            }
        }

        impl From<wgt::$wgt> for types::$wit {
            fn from(value: wgt::$wgt) -> Self {
                match value {
                    $(wgt::$wgt::$b => types::$wit::$a,)*
                }
            }
        }
    };
}

/// `From` one way, from `types::$wit` to `wgt::$wgt`, for an enum that only goes to wgpu-core.
macro_rules! to_wgpu {
    ($wit:ident => $wgt:ident { $($rows:tt)* }) => {
        rows!(to_wgpu_impl! ($wit $wgt) [] $($rows)*);
    };
}

macro_rules! to_wgpu_impl {
    (($wit:ident $wgt:ident) [$(($a:ident $b:ident))*]) => {
        impl From<types::$wit> for wgt::$wgt {
            fn from(value: types::$wit) -> Self {
                match value {
                    $(types::$wit::$a => wgt::$wgt::$b,)*
                }
            }
        }
    };
}

both_ways!(PowerPreference <=> PowerPreference { None, LowPower, HighPerformance });
both_ways!(Backend <=> Backend { Noop, Vulkan, Metal, Dx12, Gl, BrowserWebGpu });
both_ways!(DeviceType <=> DeviceType { Other, IntegratedGpu, DiscreteGpu, VirtualGpu, Cpu });
both_ways!(ShaderModel <=> ShaderModel { Sm2, Sm4, Sm5 });
both_ways!(AstcBlock <=> AstcBlock {
    B4x4, B5x4, B5x5, B6x5, B6x6, B8x5, B8x6, B8x8, B10x5, B10x6, B10x8, B10x10, B12x10, B12x12,
});
both_ways!(AstcChannel <=> AstcChannel { Unorm, UnormSrgb, Hdr });
both_ways!(TextureAspect <=> TextureAspect { All, StencilOnly, DepthOnly, Plane0, Plane1, Plane2 });
both_ways!(TextureDimension <=> TextureDimension { D1, D2, D3 });
both_ways!(TextureViewDimension <=> TextureViewDimension { D1, D2, D2Array, Cube, CubeArray, D3 });
both_ways!(DeviceLostReason <=> DeviceLostReason { Unknown, Destroyed });
both_ways!(PollStatus <=> PollStatus { QueueEmpty, WaitSucceeded, Poll });
both_ways!(PresentMode <=> PresentMode {
    AutoVsync, AutoNoVsync, Fifo, FifoRelaxed, Immediate, Mailbox,
});
both_ways!(CompositeAlphaMode <=> CompositeAlphaMode {
    Auto, Opaque, PreMultiplied, PostMultiplied, Inherit,
});
both_ways!(SurfaceColorSpace <=> SurfaceColorSpace {
    Auto, Srgb, ExtendedSrgbLinear, DisplayP3, Bt2100Pq, Bt2100Hlg, ExtendedSrgb, ExtendedDisplayP3,
});

to_wgpu!(ExternalTextureFormat => ExternalTextureFormat { Rgba, Nv12, Yu12 });
to_wgpu!(AddressMode => AddressMode { ClampToEdge, Repeat, MirrorRepeat, ClampToBorder });
to_wgpu!(FilterMode => FilterMode { Nearest, Linear });
to_wgpu!(MipmapFilterMode => MipmapFilterMode { Nearest, Linear });
to_wgpu!(SamplerBorderColor => SamplerBorderColor { TransparentBlack, OpaqueBlack, OpaqueWhite, Zero });
to_wgpu!(CompareFunction => CompareFunction {
    Never, Less, Equal, LessEqual, Greater, NotEqual, GreaterEqual, Always,
});
to_wgpu!(StorageTextureAccess => StorageTextureAccess { WriteOnly, ReadOnly, ReadWrite, Atomic });
to_wgpu!(SamplerBindingType => SamplerBindingType { Filtering, NonFiltering, Comparison });
to_wgpu!(VertexFormat => VertexFormat {
    Uint8, Uint8x2, Uint8x4, Sint8, Sint8x2, Sint8x4, Unorm8, Unorm8x2, Unorm8x4, Snorm8, Snorm8x2,
    Snorm8x4, Uint16, Uint16x2, Uint16x4, Sint16, Sint16x2, Sint16x4, Unorm16, Unorm16x2,
    Unorm16x4, Snorm16, Snorm16x2, Snorm16x4, Float16, Float16x2, Float16x4, Float32, Float32x2,
    Float32x3, Float32x4, Uint32, Uint32x2, Uint32x3, Uint32x4, Sint32, Sint32x2, Sint32x3,
    Sint32x4, Float64, Float64x2, Float64x3, Float64x4, Unorm1010102 = Unorm10_10_10_2,
    Unorm8x4Bgra,
});
to_wgpu!(VertexStepMode => VertexStepMode { Vertex, Instance });
to_wgpu!(PrimitiveTopology => PrimitiveTopology {
    PointList, LineList, LineStrip, TriangleList, TriangleStrip,
});
to_wgpu!(IndexFormat => IndexFormat { Uint16, Uint32 });
to_wgpu!(FrontFace => FrontFace { Ccw, Cw });
to_wgpu!(Face => Face { Front, Back });
to_wgpu!(PolygonMode => PolygonMode { Fill, Line, Point });
to_wgpu!(StencilOperation => StencilOperation {
    Keep, Zero, Replace, Invert, IncrementClamp, DecrementClamp, IncrementWrap, DecrementWrap,
});
to_wgpu!(BlendFactor => BlendFactor {
    Zero, One, Src, OneMinusSrc, SrcAlpha, OneMinusSrcAlpha, Dst, OneMinusDst, DstAlpha,
    OneMinusDstAlpha, SrcAlphaSaturated, Constant, OneMinusConstant, Src1, OneMinusSrc1, Src1Alpha,
    OneMinusSrc1Alpha,
});
to_wgpu!(BlendOperation => BlendOperation { Add, Subtract, ReverseSubtract, Min, Max });
to_wgpu!(StoreOp => StoreOp { Store, Discard });

/// The formats with no data, both ways; `astc`, which has a block and a channel, is written out
/// beside them.
macro_rules! texture_formats {
    ($($rows:tt)*) => {
        rows!(texture_formats_impl! () [] $($rows)*);
    };
}

macro_rules! texture_formats_impl {
    (() [$(($a:ident $b:ident))*]) => {
        impl From<types::TextureFormat> for wgt::TextureFormat {
            fn from(value: types::TextureFormat) -> Self {
                match value {
                    $(types::TextureFormat::$a => wgt::TextureFormat::$b,)*
                    types::TextureFormat::Astc((block, channel)) => wgt::TextureFormat::Astc {
                        block: block.into(),
                        channel: channel.into(),
                    },
                }
            }
        }

        impl From<wgt::TextureFormat> for types::TextureFormat {
            fn from(value: wgt::TextureFormat) -> Self {
                match value {
                    $(wgt::TextureFormat::$b => types::TextureFormat::$a,)*
                    wgt::TextureFormat::Astc { block, channel } => {
                        types::TextureFormat::Astc((block.into(), channel.into()))
                    }
                }
            }
        }
    };
}

texture_formats! {
    R8Unorm, R8Snorm, R8Uint, R8Sint, R16Uint, R16Sint, R16Unorm, R16Snorm, R16Float, Rg8Unorm,
    Rg8Snorm, Rg8Uint, Rg8Sint, R32Uint, R32Sint, R32Float, Rg16Uint, Rg16Sint, Rg16Unorm,
    Rg16Snorm, Rg16Float, Rgba8Unorm, Rgba8UnormSrgb, Rgba8Snorm, Rgba8Uint, Rgba8Sint, Bgra8Unorm,
    Bgra8UnormSrgb, Rgb9e5Ufloat, Rgb10a2Uint, Rgb10a2Unorm, Rg11b10Ufloat, R64Uint, Rg32Uint,
    Rg32Sint, Rg32Float, Rgba16Uint, Rgba16Sint, Rgba16Unorm, Rgba16Snorm, Rgba16Float, Rgba32Uint,
    Rgba32Sint, Rgba32Float, Stencil8, Depth16Unorm, Depth24Plus, Depth24PlusStencil8,
    Depth32Float, Depth32FloatStencil8, Nv12 = NV12, P010, Bc1RgbaUnorm, Bc1RgbaUnormSrgb,
    Bc2RgbaUnorm, Bc2RgbaUnormSrgb, Bc3RgbaUnorm, Bc3RgbaUnormSrgb, Bc4RUnorm, Bc4RSnorm,
    Bc5RgUnorm, Bc5RgSnorm, Bc6hRgbUfloat, Bc6hRgbFloat, Bc7RgbaUnorm, Bc7RgbaUnormSrgb,
    Etc2Rgb8Unorm, Etc2Rgb8UnormSrgb, Etc2Rgb8A1Unorm, Etc2Rgb8A1UnormSrgb, Etc2Rgba8Unorm,
    Etc2Rgba8UnormSrgb, EacR11Unorm, EacR11Snorm, EacRg11Unorm, EacRg11Snorm,
}

impl From<types::BufferBindingType> for wgt::BufferBindingType {
    fn from(value: types::BufferBindingType) -> Self {
        match value {
            types::BufferBindingType::Uniform => wgt::BufferBindingType::Uniform,
            types::BufferBindingType::Storage => {
                wgt::BufferBindingType::Storage { read_only: false }
            }
            types::BufferBindingType::ReadOnlyStorage => {
                wgt::BufferBindingType::Storage { read_only: true }
            }
        }
    }
}

impl From<types::TextureSampleType> for wgt::TextureSampleType {
    fn from(value: types::TextureSampleType) -> Self {
        match value {
            types::TextureSampleType::Float => wgt::TextureSampleType::Float { filterable: true },
            types::TextureSampleType::UnfilterableFloat => {
                wgt::TextureSampleType::Float { filterable: false }
            }
            types::TextureSampleType::Depth => wgt::TextureSampleType::Depth,
            types::TextureSampleType::Sint => wgt::TextureSampleType::Sint,
            types::TextureSampleType::Uint => wgt::TextureSampleType::Uint,
        }
    }
}

impl From<types::ShaderStage> for wgpu_core::naga::ShaderStage {
    fn from(value: types::ShaderStage) -> Self {
        match value {
            types::ShaderStage::Vertex => wgpu_core::naga::ShaderStage::Vertex,
            types::ShaderStage::Fragment => wgpu_core::naga::ShaderStage::Fragment,
            types::ShaderStage::Compute => wgpu_core::naga::ShaderStage::Compute,
        }
    }
}

impl From<types::MapMode> for wgpu_core::device::HostMap {
    fn from(value: types::MapMode) -> Self {
        match value {
            types::MapMode::Read => wgpu_core::device::HostMap::Read,
            types::MapMode::Write => wgpu_core::device::HostMap::Write,
        }
    }
}

/// A display's gamut, where the program's package names it: wgpu may name gamuts later.
pub(crate) fn display_gamut(gamut: wgt::DisplayGamut) -> Option<types::DisplayGamut> {
    match gamut {
        wgt::DisplayGamut::Srgb => Some(types::DisplayGamut::Srgb),
        wgt::DisplayGamut::DisplayP3 => Some(types::DisplayGamut::DisplayP3),
        wgt::DisplayGamut::Rec2020 => Some(types::DisplayGamut::Rec2020),
        _ => None,
    }
}
