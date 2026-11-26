//! Textures' vocabulary and geometry: formats, aspects, dimensions, usages, vertex formats,
//! stages, extents, and the descriptors of textures, views and copies.

use wasdk::gpu_types as wit;

use super::{enum_conversions, flag_conversions, label};

enum_conversions! {
    texture_format_to_wit, texture_format_from_wit: TextureFormat <=> TextureFormat {
        R8Unorm, R8Snorm, R8Uint, R8Sint, R16Uint, R16Sint, R16Unorm, R16Snorm, R16Float,
        Rg8Unorm, Rg8Snorm, Rg8Uint, Rg8Sint, R32Uint, R32Sint, R32Float, Rg16Uint, Rg16Sint,
        Rg16Unorm, Rg16Snorm, Rg16Float, Rgba8Unorm, Rgba8UnormSrgb, Rgba8Snorm, Rgba8Uint,
        Rgba8Sint, Bgra8Unorm, Bgra8UnormSrgb, Rgb9e5Ufloat, Rgb10a2Uint, Rgb10a2Unorm,
        Rg11b10Ufloat, R64Uint, Rg32Uint, Rg32Sint, Rg32Float, Rgba16Uint, Rgba16Sint,
        Rgba16Unorm, Rgba16Snorm, Rgba16Float, Rgba32Uint, Rgba32Sint, Rgba32Float, Stencil8,
        Depth16Unorm, Depth24Plus, Depth24PlusStencil8, Depth32Float, Depth32FloatStencil8,
        NV12 => Nv12, P010, Bc1RgbaUnorm, Bc1RgbaUnormSrgb, Bc2RgbaUnorm, Bc2RgbaUnormSrgb,
        Bc3RgbaUnorm, Bc3RgbaUnormSrgb, Bc4RUnorm, Bc4RSnorm, Bc5RgUnorm, Bc5RgSnorm,
        Bc6hRgbUfloat, Bc6hRgbFloat, Bc7RgbaUnorm, Bc7RgbaUnormSrgb, Etc2Rgb8Unorm,
        Etc2Rgb8UnormSrgb, Etc2Rgb8A1Unorm, Etc2Rgb8A1UnormSrgb, Etc2Rgba8Unorm,
        Etc2Rgba8UnormSrgb, EacR11Unorm, EacR11Snorm, EacRg11Unorm, EacRg11Snorm,
    }
    to_wit {
        wgpu::TextureFormat::Astc { block, channel } => {
            wit::TextureFormat::Astc((astc_block_to_wit(block), astc_channel_to_wit(channel)))
        },
    }
    from_wit {
        wit::TextureFormat::Astc((block, channel)) => wgpu::TextureFormat::Astc {
            block: astc_block_from_wit(block),
            channel: astc_channel_from_wit(channel),
        },
    }
}

enum_conversions! {
    astc_block_to_wit, astc_block_from_wit: AstcBlock <=> AstcBlock {
        B4x4, B5x4, B5x5, B6x5, B6x6, B8x5, B8x6, B8x8, B10x5, B10x6, B10x8, B10x10, B12x10,
        B12x12,
    }
}

enum_conversions! {
    astc_channel_to_wit, astc_channel_from_wit: AstcChannel <=> AstcChannel {
        Unorm, UnormSrgb, Hdr,
    }
}

enum_conversions! {
    texture_aspect_to_wit, texture_aspect_from_wit: TextureAspect <=> TextureAspect {
        All, StencilOnly, DepthOnly, Plane0, Plane1, Plane2,
    }
}

enum_conversions! {
    texture_dimension_to_wit, texture_dimension_from_wit: TextureDimension <=> TextureDimension {
        D1, D2, D3,
    }
}

enum_conversions! {
    texture_view_dimension_to_wit, texture_view_dimension_from_wit:
        TextureViewDimension <=> TextureViewDimension {
        D1, D2, D2Array, Cube, CubeArray, D3,
    }
}

flag_conversions! {
    texture_usages_to_wit, texture_usages_from_wit: TextureUsages <=> TextureUsages {
        COPY_SRC, COPY_DST, TEXTURE_BINDING, STORAGE_BINDING, RENDER_ATTACHMENT,
        TRANSIENT_ATTACHMENT, STORAGE_ATOMIC,
    }
}

flag_conversions! {
    texture_format_feature_flags_to_wit, texture_format_feature_flags_from_wit:
        TextureFormatFeatureFlags <=> TextureFormatFeatureFlags {
        FILTERABLE, MULTISAMPLE_X2, MULTISAMPLE_X4, MULTISAMPLE_X8, MULTISAMPLE_X16,
        MULTISAMPLE_RESOLVE, STORAGE_READ_ONLY, STORAGE_WRITE_ONLY, STORAGE_READ_WRITE,
        STORAGE_ATOMIC, BLENDABLE,
    }
}

flag_conversions! {
    /// A buffer's usages; ray tracing's inputs are left out.
    buffer_usages_to_wit, buffer_usages_from_wit: BufferUsages <=> BufferUsages {
        MAP_READ, MAP_WRITE, COPY_SRC, COPY_DST, INDEX, VERTEX, UNIFORM, STORAGE, INDIRECT,
        QUERY_RESOLVE,
    }
}

flag_conversions! {
    /// The stages a binding is visible to; the mesh and ray tracing stages are left out.
    shader_stages_to_wit, shader_stages_from_wit: ShaderStages <=> ShaderStages {
        VERTEX, FRAGMENT, COMPUTE,
    }
}

flag_conversions! {
    color_writes_to_wit, color_writes_from_wit: ColorWrites <=> ColorWrites {
        RED, GREEN, BLUE, ALPHA,
    }
}

enum_conversions! {
    vertex_format_to_wit, vertex_format_from_wit: VertexFormat <=> VertexFormat {
        Uint8, Uint8x2, Uint8x4, Sint8, Sint8x2, Sint8x4, Unorm8, Unorm8x2, Unorm8x4, Snorm8,
        Snorm8x2, Snorm8x4, Uint16, Uint16x2, Uint16x4, Sint16, Sint16x2, Sint16x4, Unorm16,
        Unorm16x2, Unorm16x4, Snorm16, Snorm16x2, Snorm16x4, Float16, Float16x2, Float16x4,
        Float32, Float32x2, Float32x3, Float32x4, Uint32, Uint32x2, Uint32x3, Uint32x4, Sint32,
        Sint32x2, Sint32x3, Sint32x4, Float64, Float64x2, Float64x3, Float64x4,
        Unorm10_10_10_2 => Unorm1010102, Unorm8x4Bgra,
    }
}

enum_conversions! {
    index_format_to_wit, index_format_from_wit: IndexFormat <=> IndexFormat {
        Uint16, Uint32,
    }
}

pub fn extent_to_wit(extent: wgpu::Extent3d) -> wit::Extent3d {
    wit::Extent3d {
        width: extent.width,
        height: extent.height,
        depth_or_array_layers: extent.depth_or_array_layers,
    }
}

pub fn extent_from_wit(extent: wit::Extent3d) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: extent.width,
        height: extent.height,
        depth_or_array_layers: extent.depth_or_array_layers,
    }
}

pub fn origin_to_wit(origin: wgpu::Origin3d) -> wit::Origin3d {
    wit::Origin3d {
        x: origin.x,
        y: origin.y,
        z: origin.z,
    }
}

pub fn color_to_wit(color: wgpu::Color) -> wit::Color {
    wit::Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

pub fn range_to_wit(range: std::ops::Range<u32>) -> wit::Range {
    wit::Range {
        start: range.start,
        end: range.end,
    }
}

pub fn texel_copy_layout_to_wit(layout: wgpu::TexelCopyBufferLayout) -> wit::TexelCopyBufferLayout {
    wit::TexelCopyBufferLayout {
        offset: layout.offset,
        bytes_per_row: layout.bytes_per_row,
        rows_per_image: layout.rows_per_image,
    }
}

pub fn image_subresource_range_to_wit(
    range: &wgpu::ImageSubresourceRange,
) -> wit::ImageSubresourceRange {
    wit::ImageSubresourceRange {
        aspect: texture_aspect_to_wit(range.aspect),
        base_mip_level: range.base_mip_level,
        mip_level_count: range.mip_level_count,
        base_array_layer: range.base_array_layer,
        array_layer_count: range.array_layer_count,
    }
}

fn texture_formats_to_wit(formats: &[wgpu::TextureFormat]) -> Vec<wit::TextureFormat> {
    formats.iter().copied().map(texture_format_to_wit).collect()
}

pub fn texture_descriptor_to_wit(desc: &wgpu::TextureDescriptor<'_>) -> wit::TextureDescriptor {
    wit::TextureDescriptor {
        label: label(desc.label),
        size: extent_to_wit(desc.size),
        mip_level_count: desc.mip_level_count,
        sample_count: desc.sample_count,
        dimension: texture_dimension_to_wit(desc.dimension),
        format: texture_format_to_wit(desc.format),
        usage: texture_usages_to_wit(desc.usage),
        view_formats: texture_formats_to_wit(desc.view_formats),
    }
}

pub fn texture_view_descriptor_to_wit(
    desc: &wgpu::TextureViewDescriptor<'_>,
) -> wit::TextureViewDescriptor {
    wit::TextureViewDescriptor {
        label: label(desc.label),
        format: desc.format.map(texture_format_to_wit),
        dimension: desc.dimension.map(texture_view_dimension_to_wit),
        usage: desc.usage.map(texture_usages_to_wit),
        aspect: texture_aspect_to_wit(desc.aspect),
        base_mip_level: desc.base_mip_level,
        mip_level_count: desc.mip_level_count,
        base_array_layer: desc.base_array_layer,
        array_layer_count: desc.array_layer_count,
    }
}

enum_conversions! {
    external_texture_format_to_wit, external_texture_format_from_wit:
        ExternalTextureFormat <=> ExternalTextureFormat {
        Rgba, Nv12, Yu12,
    }
}

fn transfer_function_to_wit(
    function: wgpu::ExternalTextureTransferFunction,
) -> wit::ExternalTextureTransferFunction {
    wit::ExternalTextureTransferFunction {
        a: function.a,
        b: function.b,
        g: function.g,
        k: function.k,
    }
}

pub fn external_texture_descriptor_to_wit(
    desc: &wgpu::ExternalTextureDescriptor<'_>,
) -> wit::ExternalTextureDescriptor {
    wit::ExternalTextureDescriptor {
        label: label(desc.label),
        width: desc.width,
        height: desc.height,
        format: external_texture_format_to_wit(desc.format),
        yuv_conversion_matrix: desc.yuv_conversion_matrix.to_vec(),
        gamut_conversion_matrix: desc.gamut_conversion_matrix.to_vec(),
        src_transfer_function: transfer_function_to_wit(desc.src_transfer_function),
        dst_transfer_function: transfer_function_to_wit(desc.dst_transfer_function),
        sample_transform: desc.sample_transform.to_vec(),
        load_transform: desc.load_transform.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_format_comes_back_as_itself() {
        let formats = [
            wgpu::TextureFormat::NV12,
            wgpu::TextureFormat::R64Uint,
            wgpu::TextureFormat::Astc {
                block: wgpu::AstcBlock::B10x6,
                channel: wgpu::AstcChannel::Hdr,
            },
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ];
        for format in formats {
            assert_eq!(
                texture_format_from_wit(texture_format_to_wit(format)),
                format
            );
        }
    }

    #[test]
    fn every_flag_of_the_wit_has_a_row() {
        assert_eq!(
            texture_usages_to_wit(texture_usages_from_wit(wit::TextureUsages::all())),
            wit::TextureUsages::all()
        );
        assert_eq!(
            buffer_usages_to_wit(buffer_usages_from_wit(wit::BufferUsages::all())),
            wit::BufferUsages::all()
        );
        assert_eq!(
            texture_format_feature_flags_to_wit(texture_format_feature_flags_from_wit(
                wit::TextureFormatFeatureFlags::all()
            )),
            wit::TextureFormatFeatureFlags::all()
        );
    }

    #[test]
    #[should_panic(expected = "BLAS_INPUT")]
    fn a_ray_tracing_usage_is_left_out() {
        buffer_usages_to_wit(wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::BLAS_INPUT);
    }
}
