//! Formats, usages, stages and geometry: the vocabulary both sides share by name.

use wasdk::webgpu::{
    GpuBufferUsage, GpuColor, GpuColorWrite, GpuExtent3D, GpuIndexFormat, GpuMapMode, GpuOrigin3D,
    GpuShaderStage, GpuTexelCopyBufferLayout, GpuTextureAspect, GpuTextureDimension,
    GpuTextureFormat, GpuTextureUsage, GpuTextureViewDimension, GpuVertexFormat,
};
use wgpu::{
    AstcBlock, AstcChannel, BufferUsages, ColorWrites, Features, ShaderStages, TextureFormat,
    TextureUsages, VertexFormat,
};

use crate::gap::{feature_gap, wasi_gap};

pub fn texture_format_to_wasi(format: TextureFormat) -> GpuTextureFormat {
    use GpuTextureFormat as W;
    match format {
        TextureFormat::R8Unorm => W::R8unorm,
        TextureFormat::R8Snorm => W::R8snorm,
        TextureFormat::R8Uint => W::R8uint,
        TextureFormat::R8Sint => W::R8sint,
        TextureFormat::R16Uint => W::R16uint,
        TextureFormat::R16Sint => W::R16sint,
        TextureFormat::R16Unorm => W::R16unorm,
        TextureFormat::R16Snorm => W::R16snorm,
        TextureFormat::R16Float => W::R16float,
        TextureFormat::Rg8Unorm => W::Rg8unorm,
        TextureFormat::Rg8Snorm => W::Rg8snorm,
        TextureFormat::Rg8Uint => W::Rg8uint,
        TextureFormat::Rg8Sint => W::Rg8sint,
        TextureFormat::R32Uint => W::R32uint,
        TextureFormat::R32Sint => W::R32sint,
        TextureFormat::R32Float => W::R32float,
        TextureFormat::Rg16Uint => W::Rg16uint,
        TextureFormat::Rg16Sint => W::Rg16sint,
        TextureFormat::Rg16Unorm => W::Rg16unorm,
        TextureFormat::Rg16Snorm => W::Rg16snorm,
        TextureFormat::Rg16Float => W::Rg16float,
        TextureFormat::Rgba8Unorm => W::Rgba8unorm,
        TextureFormat::Rgba8UnormSrgb => W::Rgba8unormSrgb,
        TextureFormat::Rgba8Snorm => W::Rgba8snorm,
        TextureFormat::Rgba8Uint => W::Rgba8uint,
        TextureFormat::Rgba8Sint => W::Rgba8sint,
        TextureFormat::Bgra8Unorm => W::Bgra8unorm,
        TextureFormat::Bgra8UnormSrgb => W::Bgra8unormSrgb,
        TextureFormat::Rgb9e5Ufloat => W::Rgb9e5ufloat,
        TextureFormat::Rgb10a2Uint => W::Rgb10a2uint,
        TextureFormat::Rgb10a2Unorm => W::Rgb10a2unorm,
        TextureFormat::Rg11b10Ufloat => W::Rg11b10ufloat,
        TextureFormat::Rg32Uint => W::Rg32uint,
        TextureFormat::Rg32Sint => W::Rg32sint,
        TextureFormat::Rg32Float => W::Rg32float,
        TextureFormat::Rgba16Uint => W::Rgba16uint,
        TextureFormat::Rgba16Sint => W::Rgba16sint,
        TextureFormat::Rgba16Unorm => W::Rgba16unorm,
        TextureFormat::Rgba16Snorm => W::Rgba16snorm,
        TextureFormat::Rgba16Float => W::Rgba16float,
        TextureFormat::Rgba32Uint => W::Rgba32uint,
        TextureFormat::Rgba32Sint => W::Rgba32sint,
        TextureFormat::Rgba32Float => W::Rgba32float,
        TextureFormat::Stencil8 => W::Stencil8,
        TextureFormat::Depth16Unorm => W::Depth16unorm,
        TextureFormat::Depth24Plus => W::Depth24plus,
        TextureFormat::Depth24PlusStencil8 => W::Depth24plusStencil8,
        TextureFormat::Depth32Float => W::Depth32float,
        TextureFormat::Depth32FloatStencil8 => W::Depth32floatStencil8,
        TextureFormat::Bc1RgbaUnorm => W::Bc1RgbaUnorm,
        TextureFormat::Bc1RgbaUnormSrgb => W::Bc1RgbaUnormSrgb,
        TextureFormat::Bc2RgbaUnorm => W::Bc2RgbaUnorm,
        TextureFormat::Bc2RgbaUnormSrgb => W::Bc2RgbaUnormSrgb,
        TextureFormat::Bc3RgbaUnorm => W::Bc3RgbaUnorm,
        TextureFormat::Bc3RgbaUnormSrgb => W::Bc3RgbaUnormSrgb,
        TextureFormat::Bc4RUnorm => W::Bc4RUnorm,
        TextureFormat::Bc4RSnorm => W::Bc4RSnorm,
        TextureFormat::Bc5RgUnorm => W::Bc5RgUnorm,
        TextureFormat::Bc5RgSnorm => W::Bc5RgSnorm,
        TextureFormat::Bc6hRgbUfloat => W::Bc6hRgbUfloat,
        TextureFormat::Bc6hRgbFloat => W::Bc6hRgbFloat,
        TextureFormat::Bc7RgbaUnorm => W::Bc7RgbaUnorm,
        TextureFormat::Bc7RgbaUnormSrgb => W::Bc7RgbaUnormSrgb,
        TextureFormat::Etc2Rgb8Unorm => W::Etc2Rgb8unorm,
        TextureFormat::Etc2Rgb8UnormSrgb => W::Etc2Rgb8unormSrgb,
        TextureFormat::Etc2Rgb8A1Unorm => W::Etc2Rgb8a1unorm,
        TextureFormat::Etc2Rgb8A1UnormSrgb => W::Etc2Rgb8a1unormSrgb,
        TextureFormat::Etc2Rgba8Unorm => W::Etc2Rgba8unorm,
        TextureFormat::Etc2Rgba8UnormSrgb => W::Etc2Rgba8unormSrgb,
        TextureFormat::EacR11Unorm => W::EacR11unorm,
        TextureFormat::EacR11Snorm => W::EacR11snorm,
        TextureFormat::EacRg11Unorm => W::EacRg11unorm,
        TextureFormat::EacRg11Snorm => W::EacRg11snorm,
        TextureFormat::Astc { block, channel } => astc_to_wasi(block, channel),
        TextureFormat::R64Uint | TextureFormat::NV12 | TextureFormat::P010 => {
            wasi_gap(format_args!("texture format {format:?}"))
        }
    }
}

fn astc_to_wasi(block: AstcBlock, channel: AstcChannel) -> GpuTextureFormat {
    use GpuTextureFormat as W;
    let srgb = match channel {
        AstcChannel::Unorm => false,
        AstcChannel::UnormSrgb => true,
        AstcChannel::Hdr => wasi_gap("texture format Astc { channel: Hdr }"),
    };
    match (block, srgb) {
        (AstcBlock::B4x4, false) => W::Astc4x4Unorm,
        (AstcBlock::B4x4, true) => W::Astc4x4UnormSrgb,
        (AstcBlock::B5x4, false) => W::Astc5x4Unorm,
        (AstcBlock::B5x4, true) => W::Astc5x4UnormSrgb,
        (AstcBlock::B5x5, false) => W::Astc5x5Unorm,
        (AstcBlock::B5x5, true) => W::Astc5x5UnormSrgb,
        (AstcBlock::B6x5, false) => W::Astc6x5Unorm,
        (AstcBlock::B6x5, true) => W::Astc6x5UnormSrgb,
        (AstcBlock::B6x6, false) => W::Astc6x6Unorm,
        (AstcBlock::B6x6, true) => W::Astc6x6UnormSrgb,
        (AstcBlock::B8x5, false) => W::Astc8x5Unorm,
        (AstcBlock::B8x5, true) => W::Astc8x5UnormSrgb,
        (AstcBlock::B8x6, false) => W::Astc8x6Unorm,
        (AstcBlock::B8x6, true) => W::Astc8x6UnormSrgb,
        (AstcBlock::B8x8, false) => W::Astc8x8Unorm,
        (AstcBlock::B8x8, true) => W::Astc8x8UnormSrgb,
        (AstcBlock::B10x5, false) => W::Astc10x5Unorm,
        (AstcBlock::B10x5, true) => W::Astc10x5UnormSrgb,
        (AstcBlock::B10x6, false) => W::Astc10x6Unorm,
        (AstcBlock::B10x6, true) => W::Astc10x6UnormSrgb,
        (AstcBlock::B10x8, false) => W::Astc10x8Unorm,
        (AstcBlock::B10x8, true) => W::Astc10x8UnormSrgb,
        (AstcBlock::B10x10, false) => W::Astc10x10Unorm,
        (AstcBlock::B10x10, true) => W::Astc10x10UnormSrgb,
        (AstcBlock::B12x10, false) => W::Astc12x10Unorm,
        (AstcBlock::B12x10, true) => W::Astc12x10UnormSrgb,
        (AstcBlock::B12x12, false) => W::Astc12x12Unorm,
        (AstcBlock::B12x12, true) => W::Astc12x12UnormSrgb,
    }
}

pub fn texture_format_from_wasi(format: GpuTextureFormat) -> TextureFormat {
    use GpuTextureFormat as W;
    match format {
        W::R8unorm => TextureFormat::R8Unorm,
        W::R8snorm => TextureFormat::R8Snorm,
        W::R8uint => TextureFormat::R8Uint,
        W::R8sint => TextureFormat::R8Sint,
        W::R16unorm => TextureFormat::R16Unorm,
        W::R16snorm => TextureFormat::R16Snorm,
        W::R16uint => TextureFormat::R16Uint,
        W::R16sint => TextureFormat::R16Sint,
        W::R16float => TextureFormat::R16Float,
        W::Rg8unorm => TextureFormat::Rg8Unorm,
        W::Rg8snorm => TextureFormat::Rg8Snorm,
        W::Rg8uint => TextureFormat::Rg8Uint,
        W::Rg8sint => TextureFormat::Rg8Sint,
        W::R32uint => TextureFormat::R32Uint,
        W::R32sint => TextureFormat::R32Sint,
        W::R32float => TextureFormat::R32Float,
        W::Rg16unorm => TextureFormat::Rg16Unorm,
        W::Rg16snorm => TextureFormat::Rg16Snorm,
        W::Rg16uint => TextureFormat::Rg16Uint,
        W::Rg16sint => TextureFormat::Rg16Sint,
        W::Rg16float => TextureFormat::Rg16Float,
        W::Rgba8unorm => TextureFormat::Rgba8Unorm,
        W::Rgba8unormSrgb => TextureFormat::Rgba8UnormSrgb,
        W::Rgba8snorm => TextureFormat::Rgba8Snorm,
        W::Rgba8uint => TextureFormat::Rgba8Uint,
        W::Rgba8sint => TextureFormat::Rgba8Sint,
        W::Bgra8unorm => TextureFormat::Bgra8Unorm,
        W::Bgra8unormSrgb => TextureFormat::Bgra8UnormSrgb,
        W::Rgb9e5ufloat => TextureFormat::Rgb9e5Ufloat,
        W::Rgb10a2uint => TextureFormat::Rgb10a2Uint,
        W::Rgb10a2unorm => TextureFormat::Rgb10a2Unorm,
        W::Rg11b10ufloat => TextureFormat::Rg11b10Ufloat,
        W::Rg32uint => TextureFormat::Rg32Uint,
        W::Rg32sint => TextureFormat::Rg32Sint,
        W::Rg32float => TextureFormat::Rg32Float,
        W::Rgba16unorm => TextureFormat::Rgba16Unorm,
        W::Rgba16snorm => TextureFormat::Rgba16Snorm,
        W::Rgba16uint => TextureFormat::Rgba16Uint,
        W::Rgba16sint => TextureFormat::Rgba16Sint,
        W::Rgba16float => TextureFormat::Rgba16Float,
        W::Rgba32uint => TextureFormat::Rgba32Uint,
        W::Rgba32sint => TextureFormat::Rgba32Sint,
        W::Rgba32float => TextureFormat::Rgba32Float,
        W::Stencil8 => TextureFormat::Stencil8,
        W::Depth16unorm => TextureFormat::Depth16Unorm,
        W::Depth24plus => TextureFormat::Depth24Plus,
        W::Depth24plusStencil8 => TextureFormat::Depth24PlusStencil8,
        W::Depth32float => TextureFormat::Depth32Float,
        W::Depth32floatStencil8 => TextureFormat::Depth32FloatStencil8,
        W::Bc1RgbaUnorm => TextureFormat::Bc1RgbaUnorm,
        W::Bc1RgbaUnormSrgb => TextureFormat::Bc1RgbaUnormSrgb,
        W::Bc2RgbaUnorm => TextureFormat::Bc2RgbaUnorm,
        W::Bc2RgbaUnormSrgb => TextureFormat::Bc2RgbaUnormSrgb,
        W::Bc3RgbaUnorm => TextureFormat::Bc3RgbaUnorm,
        W::Bc3RgbaUnormSrgb => TextureFormat::Bc3RgbaUnormSrgb,
        W::Bc4RUnorm => TextureFormat::Bc4RUnorm,
        W::Bc4RSnorm => TextureFormat::Bc4RSnorm,
        W::Bc5RgUnorm => TextureFormat::Bc5RgUnorm,
        W::Bc5RgSnorm => TextureFormat::Bc5RgSnorm,
        W::Bc6hRgbUfloat => TextureFormat::Bc6hRgbUfloat,
        W::Bc6hRgbFloat => TextureFormat::Bc6hRgbFloat,
        W::Bc7RgbaUnorm => TextureFormat::Bc7RgbaUnorm,
        W::Bc7RgbaUnormSrgb => TextureFormat::Bc7RgbaUnormSrgb,
        W::Etc2Rgb8unorm => TextureFormat::Etc2Rgb8Unorm,
        W::Etc2Rgb8unormSrgb => TextureFormat::Etc2Rgb8UnormSrgb,
        W::Etc2Rgb8a1unorm => TextureFormat::Etc2Rgb8A1Unorm,
        W::Etc2Rgb8a1unormSrgb => TextureFormat::Etc2Rgb8A1UnormSrgb,
        W::Etc2Rgba8unorm => TextureFormat::Etc2Rgba8Unorm,
        W::Etc2Rgba8unormSrgb => TextureFormat::Etc2Rgba8UnormSrgb,
        W::EacR11unorm => TextureFormat::EacR11Unorm,
        W::EacR11snorm => TextureFormat::EacR11Snorm,
        W::EacRg11unorm => TextureFormat::EacRg11Unorm,
        W::EacRg11snorm => TextureFormat::EacRg11Snorm,
        W::Astc4x4Unorm => astc(AstcBlock::B4x4, AstcChannel::Unorm),
        W::Astc4x4UnormSrgb => astc(AstcBlock::B4x4, AstcChannel::UnormSrgb),
        W::Astc5x4Unorm => astc(AstcBlock::B5x4, AstcChannel::Unorm),
        W::Astc5x4UnormSrgb => astc(AstcBlock::B5x4, AstcChannel::UnormSrgb),
        W::Astc5x5Unorm => astc(AstcBlock::B5x5, AstcChannel::Unorm),
        W::Astc5x5UnormSrgb => astc(AstcBlock::B5x5, AstcChannel::UnormSrgb),
        W::Astc6x5Unorm => astc(AstcBlock::B6x5, AstcChannel::Unorm),
        W::Astc6x5UnormSrgb => astc(AstcBlock::B6x5, AstcChannel::UnormSrgb),
        W::Astc6x6Unorm => astc(AstcBlock::B6x6, AstcChannel::Unorm),
        W::Astc6x6UnormSrgb => astc(AstcBlock::B6x6, AstcChannel::UnormSrgb),
        W::Astc8x5Unorm => astc(AstcBlock::B8x5, AstcChannel::Unorm),
        W::Astc8x5UnormSrgb => astc(AstcBlock::B8x5, AstcChannel::UnormSrgb),
        W::Astc8x6Unorm => astc(AstcBlock::B8x6, AstcChannel::Unorm),
        W::Astc8x6UnormSrgb => astc(AstcBlock::B8x6, AstcChannel::UnormSrgb),
        W::Astc8x8Unorm => astc(AstcBlock::B8x8, AstcChannel::Unorm),
        W::Astc8x8UnormSrgb => astc(AstcBlock::B8x8, AstcChannel::UnormSrgb),
        W::Astc10x5Unorm => astc(AstcBlock::B10x5, AstcChannel::Unorm),
        W::Astc10x5UnormSrgb => astc(AstcBlock::B10x5, AstcChannel::UnormSrgb),
        W::Astc10x6Unorm => astc(AstcBlock::B10x6, AstcChannel::Unorm),
        W::Astc10x6UnormSrgb => astc(AstcBlock::B10x6, AstcChannel::UnormSrgb),
        W::Astc10x8Unorm => astc(AstcBlock::B10x8, AstcChannel::Unorm),
        W::Astc10x8UnormSrgb => astc(AstcBlock::B10x8, AstcChannel::UnormSrgb),
        W::Astc10x10Unorm => astc(AstcBlock::B10x10, AstcChannel::Unorm),
        W::Astc10x10UnormSrgb => astc(AstcBlock::B10x10, AstcChannel::UnormSrgb),
        W::Astc12x10Unorm => astc(AstcBlock::B12x10, AstcChannel::Unorm),
        W::Astc12x10UnormSrgb => astc(AstcBlock::B12x10, AstcChannel::UnormSrgb),
        W::Astc12x12Unorm => astc(AstcBlock::B12x12, AstcChannel::Unorm),
        W::Astc12x12UnormSrgb => astc(AstcBlock::B12x12, AstcChannel::UnormSrgb),
    }
}

fn astc(block: AstcBlock, channel: AstcChannel) -> TextureFormat {
    TextureFormat::Astc { block, channel }
}

const TEXTURE_USAGES: &[(TextureUsages, GpuTextureUsage)] = &[
    (TextureUsages::COPY_SRC, GpuTextureUsage::COPY_SRC),
    (TextureUsages::COPY_DST, GpuTextureUsage::COPY_DST),
    (
        TextureUsages::TEXTURE_BINDING,
        GpuTextureUsage::TEXTURE_BINDING,
    ),
    (
        TextureUsages::STORAGE_BINDING,
        GpuTextureUsage::STORAGE_BINDING,
    ),
    (
        TextureUsages::RENDER_ATTACHMENT,
        GpuTextureUsage::RENDER_ATTACHMENT,
    ),
    (
        TextureUsages::TRANSIENT_ATTACHMENT,
        GpuTextureUsage::TRANSIENT_ATTACHMENT,
    ),
];

pub fn texture_usages_to_wasi(usages: TextureUsages) -> GpuTextureUsage {
    // A hint that the attachment need not persist past its pass. The interface names it,
    // but the host's wgpu-core rejects it; wgpu's browser backend strips it too.
    let usages = usages.difference(TextureUsages::TRANSIENT_ATTACHMENT);
    let known = TEXTURE_USAGES
        .iter()
        .fold(TextureUsages::empty(), |known, (from, _)| known | *from);
    let unknown = usages.difference(known);
    if !unknown.is_empty() {
        wasi_gap(format_args!("texture usages {unknown:?}"));
    }
    TEXTURE_USAGES
        .iter()
        .filter(|(from, _)| usages.contains(*from))
        .fold(GpuTextureUsage::empty(), |out, (_, to)| out | *to)
}

pub fn texture_usages_from_wasi(usage: GpuTextureUsage) -> TextureUsages {
    TEXTURE_USAGES
        .iter()
        .filter(|(_, from)| usage.contains(*from))
        .fold(TextureUsages::empty(), |out, (to, _)| out | *to)
}

const BUFFER_USAGES: &[(BufferUsages, GpuBufferUsage)] = &[
    (BufferUsages::MAP_READ, GpuBufferUsage::MAP_READ),
    (BufferUsages::MAP_WRITE, GpuBufferUsage::MAP_WRITE),
    (BufferUsages::COPY_SRC, GpuBufferUsage::COPY_SRC),
    (BufferUsages::COPY_DST, GpuBufferUsage::COPY_DST),
    (BufferUsages::INDEX, GpuBufferUsage::INDEX),
    (BufferUsages::VERTEX, GpuBufferUsage::VERTEX),
    (BufferUsages::UNIFORM, GpuBufferUsage::UNIFORM),
    (BufferUsages::STORAGE, GpuBufferUsage::STORAGE),
    (BufferUsages::INDIRECT, GpuBufferUsage::INDIRECT),
    (BufferUsages::QUERY_RESOLVE, GpuBufferUsage::QUERY_RESOLVE),
];

pub fn buffer_usages_to_wasi(usages: BufferUsages) -> GpuBufferUsage {
    let known = BUFFER_USAGES
        .iter()
        .fold(BufferUsages::empty(), |known, (from, _)| known | *from);
    let unknown = usages.difference(known);
    if !unknown.is_empty() {
        wasi_gap(format_args!("buffer usages {unknown:?}"));
    }
    BUFFER_USAGES
        .iter()
        .filter(|(from, _)| usages.contains(*from))
        .fold(GpuBufferUsage::empty(), |out, (_, to)| out | *to)
}

pub fn buffer_usages_from_wasi(usage: GpuBufferUsage) -> BufferUsages {
    BUFFER_USAGES
        .iter()
        .filter(|(_, from)| usage.contains(*from))
        .fold(BufferUsages::empty(), |out, (to, _)| out | *to)
}

const SHADER_STAGES: &[(ShaderStages, GpuShaderStage)] = &[
    (ShaderStages::VERTEX, GpuShaderStage::VERTEX),
    (ShaderStages::FRAGMENT, GpuShaderStage::FRAGMENT),
    (ShaderStages::COMPUTE, GpuShaderStage::COMPUTE),
];

pub fn shader_stages_to_wasi(stages: ShaderStages) -> GpuShaderStage {
    let known = SHADER_STAGES
        .iter()
        .fold(ShaderStages::empty(), |known, (from, _)| known | *from);
    let unknown = stages.difference(known);
    if !unknown.is_empty() {
        wasi_gap(format_args!("shader stages {unknown:?}"));
    }
    SHADER_STAGES
        .iter()
        .filter(|(from, _)| stages.contains(*from))
        .fold(GpuShaderStage::empty(), |out, (_, to)| out | *to)
}

const COLOR_WRITES: &[(ColorWrites, GpuColorWrite)] = &[
    (ColorWrites::RED, GpuColorWrite::RED),
    (ColorWrites::GREEN, GpuColorWrite::GREEN),
    (ColorWrites::BLUE, GpuColorWrite::BLUE),
    (ColorWrites::ALPHA, GpuColorWrite::ALPHA),
];

/// The WIT gives `all` its own bit next to the four channels; it is set alongside them when
/// every channel is written, so either reading of the flags sees the same mask.
pub fn color_writes_to_wasi(writes: ColorWrites) -> GpuColorWrite {
    let channels = COLOR_WRITES
        .iter()
        .filter(|(from, _)| writes.contains(*from))
        .fold(GpuColorWrite::empty(), |out, (_, to)| out | *to);
    if writes.contains(ColorWrites::ALL) {
        channels | GpuColorWrite::ALL
    } else {
        channels
    }
}

pub fn map_mode_to_wasi(mode: wgpu::MapMode) -> GpuMapMode {
    match mode {
        wgpu::MapMode::Read => GpuMapMode::READ,
        wgpu::MapMode::Write => GpuMapMode::WRITE,
    }
}

pub fn texture_dimension_to_wasi(dimension: wgpu::TextureDimension) -> GpuTextureDimension {
    match dimension {
        wgpu::TextureDimension::D1 => GpuTextureDimension::D1,
        wgpu::TextureDimension::D2 => GpuTextureDimension::D2,
        wgpu::TextureDimension::D3 => GpuTextureDimension::D3,
    }
}

pub fn texture_dimension_from_wasi(dimension: GpuTextureDimension) -> wgpu::TextureDimension {
    match dimension {
        GpuTextureDimension::D1 => wgpu::TextureDimension::D1,
        GpuTextureDimension::D2 => wgpu::TextureDimension::D2,
        GpuTextureDimension::D3 => wgpu::TextureDimension::D3,
    }
}

pub fn texture_view_dimension_to_wasi(
    dimension: wgpu::TextureViewDimension,
) -> GpuTextureViewDimension {
    match dimension {
        wgpu::TextureViewDimension::D1 => GpuTextureViewDimension::D1,
        wgpu::TextureViewDimension::D2 => GpuTextureViewDimension::D2,
        wgpu::TextureViewDimension::D2Array => GpuTextureViewDimension::D2Array,
        wgpu::TextureViewDimension::Cube => GpuTextureViewDimension::Cube,
        wgpu::TextureViewDimension::CubeArray => GpuTextureViewDimension::CubeArray,
        wgpu::TextureViewDimension::D3 => GpuTextureViewDimension::D3,
    }
}

pub fn texture_aspect_to_wasi(aspect: wgpu::TextureAspect) -> GpuTextureAspect {
    match aspect {
        wgpu::TextureAspect::All => GpuTextureAspect::All,
        wgpu::TextureAspect::StencilOnly => GpuTextureAspect::StencilOnly,
        wgpu::TextureAspect::DepthOnly => GpuTextureAspect::DepthOnly,
        wgpu::TextureAspect::Plane0 | wgpu::TextureAspect::Plane1 | wgpu::TextureAspect::Plane2 => {
            wasi_gap(format_args!("multi-planar texture aspect {aspect:?}"))
        }
    }
}

pub fn vertex_format_to_wasi(format: VertexFormat) -> GpuVertexFormat {
    use GpuVertexFormat as W;
    match format {
        VertexFormat::Uint8 => W::Uint8,
        VertexFormat::Uint8x2 => W::Uint8x2,
        VertexFormat::Uint8x4 => W::Uint8x4,
        VertexFormat::Sint8 => W::Sint8,
        VertexFormat::Sint8x2 => W::Sint8x2,
        VertexFormat::Sint8x4 => W::Sint8x4,
        VertexFormat::Unorm8 => W::Unorm8,
        VertexFormat::Unorm8x2 => W::Unorm8x2,
        VertexFormat::Unorm8x4 => W::Unorm8x4,
        VertexFormat::Snorm8 => W::Snorm8,
        VertexFormat::Snorm8x2 => W::Snorm8x2,
        VertexFormat::Snorm8x4 => W::Snorm8x4,
        VertexFormat::Uint16 => W::Uint16,
        VertexFormat::Uint16x2 => W::Uint16x2,
        VertexFormat::Uint16x4 => W::Uint16x4,
        VertexFormat::Sint16 => W::Sint16,
        VertexFormat::Sint16x2 => W::Sint16x2,
        VertexFormat::Sint16x4 => W::Sint16x4,
        VertexFormat::Unorm16 => W::Unorm16,
        VertexFormat::Unorm16x2 => W::Unorm16x2,
        VertexFormat::Unorm16x4 => W::Unorm16x4,
        VertexFormat::Snorm16 => W::Snorm16,
        VertexFormat::Snorm16x2 => W::Snorm16x2,
        VertexFormat::Snorm16x4 => W::Snorm16x4,
        VertexFormat::Float16 => W::Float16,
        VertexFormat::Float16x2 => W::Float16x2,
        VertexFormat::Float16x4 => W::Float16x4,
        VertexFormat::Float32 => W::Float32,
        VertexFormat::Float32x2 => W::Float32x2,
        VertexFormat::Float32x3 => W::Float32x3,
        VertexFormat::Float32x4 => W::Float32x4,
        VertexFormat::Uint32 => W::Uint32,
        VertexFormat::Uint32x2 => W::Uint32x2,
        VertexFormat::Uint32x3 => W::Uint32x3,
        VertexFormat::Uint32x4 => W::Uint32x4,
        VertexFormat::Sint32 => W::Sint32,
        VertexFormat::Sint32x2 => W::Sint32x2,
        VertexFormat::Sint32x3 => W::Sint32x3,
        VertexFormat::Sint32x4 => W::Sint32x4,
        VertexFormat::Unorm10_10_10_2 => W::Unorm1010102,
        VertexFormat::Unorm8x4Bgra => W::Unorm8x4Bgra,
        VertexFormat::Float64
        | VertexFormat::Float64x2
        | VertexFormat::Float64x3
        | VertexFormat::Float64x4 => feature_gap(
            Features::VERTEX_ATTRIBUTE_64BIT,
            format_args!("vertex format {format:?}"),
        ),
    }
}

pub fn index_format_to_wasi(format: wgpu::IndexFormat) -> GpuIndexFormat {
    match format {
        wgpu::IndexFormat::Uint16 => GpuIndexFormat::Uint16,
        wgpu::IndexFormat::Uint32 => GpuIndexFormat::Uint32,
    }
}

pub fn extent_to_wasi(extent: wgpu::Extent3d) -> GpuExtent3D {
    GpuExtent3D {
        width: extent.width,
        height: Some(extent.height),
        depth_or_array_layers: Some(extent.depth_or_array_layers),
    }
}

pub fn origin_to_wasi(origin: wgpu::Origin3d) -> GpuOrigin3D {
    GpuOrigin3D {
        x: Some(origin.x),
        y: Some(origin.y),
        z: Some(origin.z),
    }
}

pub fn color_to_wasi(color: wgpu::Color) -> GpuColor {
    GpuColor {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

pub fn texel_copy_layout_to_wasi(layout: wgpu::TexelCopyBufferLayout) -> GpuTexelCopyBufferLayout {
    GpuTexelCopyBufferLayout {
        offset: Some(layout.offset),
        bytes_per_row: layout.bytes_per_row,
        rows_per_image: layout.rows_per_image,
    }
}
