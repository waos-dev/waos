//! The records and variants that hold no GPU object, field by field: to wgpu-types and
//! wgpu-core's descriptors for what a program describes, back for what wgpu answers.

use std::borrow::Cow;
use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use wgpu_types as wgt;

use super::cases::display_gamut;
use crate::textures::TextureDescriptor;
use crate::wasdk::gpu::types;

/// A label as wgpu-core takes it.
pub(crate) fn label(label: Option<String>) -> wgpu_core::Label<'static> {
    label.map(Cow::Owned)
}

impl From<types::Extent3d> for wgt::Extent3d {
    fn from(size: types::Extent3d) -> Self {
        wgt::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: size.depth_or_array_layers,
        }
    }
}

impl From<wgt::Extent3d> for types::Extent3d {
    fn from(size: wgt::Extent3d) -> Self {
        types::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: size.depth_or_array_layers,
        }
    }
}

impl From<types::Origin3d> for wgt::Origin3d {
    fn from(origin: types::Origin3d) -> Self {
        wgt::Origin3d {
            x: origin.x,
            y: origin.y,
            z: origin.z,
        }
    }
}

impl From<types::Color> for wgt::Color {
    fn from(color: types::Color) -> Self {
        wgt::Color {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        }
    }
}

impl From<wgt::AdapterInfo> for types::AdapterInfo {
    fn from(info: wgt::AdapterInfo) -> Self {
        types::AdapterInfo {
            name: info.name,
            vendor: info.vendor,
            device: info.device,
            device_type: info.device_type.into(),
            device_pci_bus_id: info.device_pci_bus_id,
            driver: info.driver,
            driver_info: info.driver_info,
            backend: info.backend.into(),
            subgroup_min_size: info.subgroup_min_size,
            subgroup_max_size: info.subgroup_max_size,
            transient_saves_memory: info.transient_saves_memory,
        }
    }
}

impl From<wgt::DownlevelCapabilities> for types::DownlevelCapabilities {
    fn from(capabilities: wgt::DownlevelCapabilities) -> Self {
        types::DownlevelCapabilities {
            flags: capabilities.flags.into(),
            shader_model: capabilities.shader_model.into(),
        }
    }
}

impl From<wgt::TextureFormatFeatures> for types::TextureFormatFeatures {
    fn from(features: wgt::TextureFormatFeatures) -> Self {
        types::TextureFormatFeatures {
            allowed_usages: features.allowed_usages.into(),
            flags: features.flags.into(),
        }
    }
}

impl From<types::MemoryHints> for wgt::MemoryHints {
    fn from(hints: types::MemoryHints) -> Self {
        match hints {
            types::MemoryHints::Performance => wgt::MemoryHints::Performance,
            types::MemoryHints::MemoryUsage => wgt::MemoryHints::MemoryUsage,
            types::MemoryHints::Manual((smallest, largest)) => wgt::MemoryHints::Manual {
                suballocated_device_memory_block_size: smallest..largest,
            },
        }
    }
}

impl From<types::TextureDescriptor> for TextureDescriptor {
    fn from(descriptor: types::TextureDescriptor) -> Self {
        wgt::TextureDescriptor {
            label: descriptor.label,
            size: descriptor.size.into(),
            mip_level_count: descriptor.mip_level_count,
            sample_count: descriptor.sample_count,
            dimension: descriptor.dimension.into(),
            format: descriptor.format.into(),
            usage: descriptor.usage.into(),
            view_formats: descriptor
                .view_formats
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<&TextureDescriptor> for types::TextureDescriptor {
    fn from(descriptor: &TextureDescriptor) -> Self {
        types::TextureDescriptor {
            label: descriptor.label.clone(),
            size: descriptor.size.into(),
            mip_level_count: descriptor.mip_level_count,
            sample_count: descriptor.sample_count,
            dimension: descriptor.dimension.into(),
            format: descriptor.format.into(),
            usage: descriptor.usage.into(),
            view_formats: descriptor
                .view_formats
                .iter()
                .map(|&format| format.into())
                .collect(),
        }
    }
}

impl From<types::TextureViewDescriptor> for wgpu_core::resource::TextureViewDescriptor<'static> {
    fn from(descriptor: types::TextureViewDescriptor) -> Self {
        wgpu_core::resource::TextureViewDescriptor {
            label: label(descriptor.label),
            format: descriptor.format.map(Into::into),
            dimension: descriptor.dimension.map(Into::into),
            usage: descriptor.usage.map(Into::into),
            range: wgt::ImageSubresourceRange {
                aspect: descriptor.aspect.into(),
                base_mip_level: descriptor.base_mip_level,
                mip_level_count: descriptor.mip_level_count,
                base_array_layer: descriptor.base_array_layer,
                array_layer_count: descriptor.array_layer_count,
            },
        }
    }
}

impl From<types::ImageSubresourceRange> for wgt::ImageSubresourceRange {
    fn from(range: types::ImageSubresourceRange) -> Self {
        wgt::ImageSubresourceRange {
            aspect: range.aspect.into(),
            base_mip_level: range.base_mip_level,
            mip_level_count: range.mip_level_count,
            base_array_layer: range.base_array_layer,
            array_layer_count: range.array_layer_count,
        }
    }
}

impl From<types::TexelCopyBufferLayout> for wgt::TexelCopyBufferLayout {
    fn from(layout: types::TexelCopyBufferLayout) -> Self {
        wgt::TexelCopyBufferLayout {
            offset: layout.offset,
            bytes_per_row: layout.bytes_per_row,
            rows_per_image: layout.rows_per_image,
        }
    }
}

impl From<types::ExternalTextureTransferFunction> for wgt::ExternalTextureTransferFunction {
    fn from(function: types::ExternalTextureTransferFunction) -> Self {
        wgt::ExternalTextureTransferFunction {
            a: function.a,
            b: function.b,
            g: function.g,
            k: function.k,
        }
    }
}

/// An external texture's descriptor, or why it is none: a matrix of the wrong size, which wgpu's
/// fixed-size arrays cannot hold.
pub(crate) fn external_texture_descriptor(
    descriptor: types::ExternalTextureDescriptor,
) -> wasmtime::Result<wgt::ExternalTextureDescriptor<wgpu_core::Label<'static>>> {
    Ok(wgt::ExternalTextureDescriptor {
        label: label(descriptor.label),
        width: descriptor.width,
        height: descriptor.height,
        format: descriptor.format.into(),
        yuv_conversion_matrix: matrix(descriptor.yuv_conversion_matrix, "yuv-conversion-matrix")?,
        gamut_conversion_matrix: matrix(
            descriptor.gamut_conversion_matrix,
            "gamut-conversion-matrix",
        )?,
        src_transfer_function: descriptor.src_transfer_function.into(),
        dst_transfer_function: descriptor.dst_transfer_function.into(),
        sample_transform: matrix(descriptor.sample_transform, "sample-transform")?,
        load_transform: matrix(descriptor.load_transform, "load-transform")?,
    })
}

fn matrix<const N: usize>(values: Vec<f32>, name: &str) -> wasmtime::Result<[f32; N]> {
    let given = values.len();
    values
        .try_into()
        .map_err(|_| wasmtime::format_err!("`{name}` has {given} values, not {N}"))
}

impl From<types::SamplerDescriptor> for wgpu_core::resource::SamplerDescriptor<'static> {
    fn from(descriptor: types::SamplerDescriptor) -> Self {
        wgpu_core::resource::SamplerDescriptor {
            label: label(descriptor.label),
            address_modes: [
                descriptor.address_mode_u.into(),
                descriptor.address_mode_v.into(),
                descriptor.address_mode_w.into(),
            ],
            mag_filter: descriptor.mag_filter.into(),
            min_filter: descriptor.min_filter.into(),
            mipmap_filter: descriptor.mipmap_filter.into(),
            lod_min_clamp: descriptor.lod_min_clamp,
            lod_max_clamp: descriptor.lod_max_clamp,
            compare: descriptor.compare.map(Into::into),
            anisotropy_clamp: descriptor.anisotropy_clamp,
            border_color: descriptor.border_color.map(Into::into),
        }
    }
}

impl From<types::BufferDescriptor> for wgpu_core::resource::BufferDescriptor<'static> {
    fn from(descriptor: types::BufferDescriptor) -> Self {
        wgt::BufferDescriptor {
            label: label(descriptor.label),
            size: descriptor.size,
            usage: descriptor.usage.into(),
            mapped_at_creation: descriptor.mapped_at_creation,
        }
    }
}

impl From<types::BindingType> for wgt::BindingType {
    fn from(ty: types::BindingType) -> Self {
        match ty {
            types::BindingType::Buffer(layout) => wgt::BindingType::Buffer {
                ty: layout.ty.into(),
                has_dynamic_offset: layout.has_dynamic_offset,
                min_binding_size: layout.min_binding_size.and_then(NonZeroU64::new),
            },
            types::BindingType::Sampler(ty) => wgt::BindingType::Sampler(ty.into()),
            types::BindingType::Texture(layout) => wgt::BindingType::Texture {
                sample_type: layout.sample_type.into(),
                view_dimension: layout.view_dimension.into(),
                multisampled: layout.multisampled,
            },
            types::BindingType::StorageTexture(layout) => wgt::BindingType::StorageTexture {
                access: layout.access.into(),
                format: layout.format.into(),
                view_dimension: layout.view_dimension.into(),
            },
            types::BindingType::ExternalTexture => wgt::BindingType::ExternalTexture,
        }
    }
}

impl From<types::BindGroupLayoutDescriptor>
    for wgpu_core::binding_model::BindGroupLayoutDescriptor<'static>
{
    fn from(descriptor: types::BindGroupLayoutDescriptor) -> Self {
        let entries = descriptor
            .entries
            .into_iter()
            .map(|entry| wgt::BindGroupLayoutEntry {
                binding: entry.binding,
                visibility: entry.visibility.into(),
                ty: entry.ty.into(),
                count: entry.count.and_then(NonZeroU32::new),
            })
            .collect::<Vec<_>>();
        wgpu_core::binding_model::BindGroupLayoutDescriptor {
            label: label(descriptor.label),
            entries: Cow::Owned(entries),
        }
    }
}

impl From<types::VertexBufferLayout> for wgpu_core::pipeline::VertexBufferLayout<'static> {
    fn from(layout: types::VertexBufferLayout) -> Self {
        let attributes = layout
            .attributes
            .into_iter()
            .map(|attribute| wgt::VertexAttribute {
                format: attribute.format.into(),
                offset: attribute.offset,
                shader_location: attribute.shader_location,
            })
            .collect::<Vec<_>>();
        wgpu_core::pipeline::VertexBufferLayout {
            array_stride: layout.array_stride,
            step_mode: layout.step_mode.into(),
            attributes: Cow::Owned(attributes),
        }
    }
}

impl From<types::PrimitiveState> for wgt::PrimitiveState {
    fn from(state: types::PrimitiveState) -> Self {
        wgt::PrimitiveState {
            topology: state.topology.into(),
            strip_index_format: state.strip_index_format.map(Into::into),
            front_face: state.front_face.into(),
            cull_mode: state.cull_mode.map(Into::into),
            unclipped_depth: state.unclipped_depth,
            polygon_mode: state.polygon_mode.into(),
            conservative: state.conservative,
        }
    }
}

impl From<types::StencilFaceState> for wgt::StencilFaceState {
    fn from(state: types::StencilFaceState) -> Self {
        wgt::StencilFaceState {
            compare: state.compare.into(),
            fail_op: state.fail_op.into(),
            depth_fail_op: state.depth_fail_op.into(),
            pass_op: state.pass_op.into(),
        }
    }
}

impl From<types::DepthStencilState> for wgt::DepthStencilState {
    fn from(state: types::DepthStencilState) -> Self {
        wgt::DepthStencilState {
            format: state.format.into(),
            depth_write_enabled: state.depth_write_enabled,
            depth_compare: state.depth_compare.map(Into::into),
            stencil: wgt::StencilState {
                front: state.stencil.front.into(),
                back: state.stencil.back.into(),
                read_mask: state.stencil.read_mask,
                write_mask: state.stencil.write_mask,
            },
            bias: wgt::DepthBiasState {
                constant: state.bias.constant,
                slope_scale: state.bias.slope_scale,
                clamp: state.bias.clamp,
            },
        }
    }
}

impl From<types::MultisampleState> for wgt::MultisampleState {
    fn from(state: types::MultisampleState) -> Self {
        wgt::MultisampleState {
            count: state.count,
            mask: state.mask,
            alpha_to_coverage_enabled: state.alpha_to_coverage_enabled,
        }
    }
}

impl From<types::BlendComponent> for wgt::BlendComponent {
    fn from(component: types::BlendComponent) -> Self {
        wgt::BlendComponent {
            src_factor: component.src_factor.into(),
            dst_factor: component.dst_factor.into(),
            operation: component.operation.into(),
        }
    }
}

impl From<types::ColorTargetState> for wgt::ColorTargetState {
    fn from(state: types::ColorTargetState) -> Self {
        wgt::ColorTargetState {
            format: state.format.into(),
            blend: state.blend.map(|blend| wgt::BlendState {
                color: blend.color.into(),
                alpha: blend.alpha.into(),
            }),
            write_mask: state.write_mask.into(),
        }
    }
}

impl From<types::ShaderSource> for wgpu_core::pipeline::ShaderModuleSource<'static> {
    fn from(source: types::ShaderSource) -> Self {
        use wgpu_core::naga::front;
        match source {
            types::ShaderSource::Wgsl(code) => {
                wgpu_core::pipeline::ShaderModuleSource::Wgsl(Cow::Owned(code))
            }
            // As wgpu reads SPIR-V: with the coordinate space as given, every capability checked.
            types::ShaderSource::Spirv(words) => wgpu_core::pipeline::ShaderModuleSource::SpirV(
                Cow::Owned(words),
                front::spv::Options {
                    adjust_coordinate_space: false,
                    strict_capabilities: true,
                    block_ctx_dump_prefix: None,
                },
            ),
            types::ShaderSource::Glsl(glsl) => wgpu_core::pipeline::ShaderModuleSource::Glsl(
                Cow::Owned(glsl.code),
                front::glsl::Options {
                    stage: glsl.stage.into(),
                    defines: glsl.defines.into_iter().collect(),
                },
            ),
        }
    }
}

impl From<types::RenderBundleEncoderDescriptor>
    for wgpu_core::command::RenderBundleEncoderDescriptor<'static>
{
    fn from(descriptor: types::RenderBundleEncoderDescriptor) -> Self {
        let color_formats = descriptor
            .color_formats
            .into_iter()
            .map(|format| format.map(Into::into))
            .collect::<Vec<_>>();
        wgpu_core::command::RenderBundleEncoderDescriptor {
            label: label(descriptor.label),
            color_formats: Cow::Owned(color_formats),
            depth_stencil: descriptor.depth_stencil.map(|depth_stencil| {
                wgt::RenderBundleDepthStencil {
                    format: depth_stencil.format.into(),
                    depth_read_only: depth_stencil.depth_read_only,
                    stencil_read_only: depth_stencil.stencil_read_only,
                }
            }),
            sample_count: descriptor.sample_count,
            multiview: descriptor.multiview.and_then(NonZeroU32::new),
        }
    }
}

impl From<types::QuerySetDescriptor> for wgpu_core::resource::QuerySetDescriptor<'static> {
    fn from(descriptor: types::QuerySetDescriptor) -> Self {
        wgt::QuerySetDescriptor {
            label: label(descriptor.label),
            ty: match descriptor.ty {
                types::QueryType::Occlusion => wgt::QueryType::Occlusion,
                types::QueryType::Timestamp => wgt::QueryType::Timestamp,
                types::QueryType::PipelineStatistics(counted) => {
                    wgt::QueryType::PipelineStatistics(counted.into())
                }
            },
            count: descriptor.count,
        }
    }
}

impl From<types::ColorOperations> for (wgt::LoadOp<wgt::Color>, wgt::StoreOp) {
    fn from(operations: types::ColorOperations) -> Self {
        let load = match operations.load {
            types::ColorLoadOp::Clear(color) => wgt::LoadOp::Clear(color.into()),
            types::ColorLoadOp::Load => wgt::LoadOp::Load,
        };
        (load, operations.store.into())
    }
}

/// A depth attachment's channel as wgpu-core takes it: read only where the pass has no
/// operations for it.
pub(crate) fn depth_channel(
    operations: Option<types::DepthOperations>,
) -> wgpu_core::command::PassChannel<Option<f32>> {
    match operations {
        Some(operations) => wgpu_core::command::PassChannel {
            load_op: Some(match operations.load {
                types::DepthLoadOp::Clear(depth) => wgt::LoadOp::Clear(Some(depth)),
                types::DepthLoadOp::Load => wgt::LoadOp::Load,
            }),
            store_op: Some(operations.store.into()),
            read_only: false,
        },
        None => read_only_channel(),
    }
}

/// A stencil attachment's channel, as [`depth_channel`].
pub(crate) fn stencil_channel(
    operations: Option<types::StencilOperations>,
) -> wgpu_core::command::PassChannel<Option<u32>> {
    match operations {
        Some(operations) => wgpu_core::command::PassChannel {
            load_op: Some(match operations.load {
                types::StencilLoadOp::Clear(stencil) => wgt::LoadOp::Clear(Some(stencil)),
                types::StencilLoadOp::Load => wgt::LoadOp::Load,
            }),
            store_op: Some(operations.store.into()),
            read_only: false,
        },
        None => read_only_channel(),
    }
}

fn read_only_channel<V>() -> wgpu_core::command::PassChannel<Option<V>> {
    wgpu_core::command::PassChannel {
        load_op: None,
        store_op: None,
        read_only: true,
    }
}

impl From<types::PollType> for wgt::PollType<wgpu_core::SubmissionIndex> {
    fn from(kind: types::PollType) -> Self {
        match kind {
            types::PollType::Poll => wgt::PollType::Poll,
            types::PollType::Wait((submission_index, timeout)) => wgt::PollType::Wait {
                submission_index,
                timeout: timeout.map(Duration::from_nanos),
            },
        }
    }
}

impl From<wgt::PollError> for types::PollError {
    fn from(error: wgt::PollError) -> Self {
        match error {
            wgt::PollError::Timeout => types::PollError::Timeout,
            wgt::PollError::WrongSubmissionIndex(asked, last) => {
                types::PollError::WrongSubmissionIndex((asked, last))
            }
        }
    }
}

impl From<types::SurfaceConfiguration> for wgt::SurfaceConfiguration<Vec<wgt::TextureFormat>> {
    fn from(configuration: types::SurfaceConfiguration) -> Self {
        wgt::SurfaceConfiguration {
            usage: configuration.usage.into(),
            format: configuration.format.into(),
            color_space: configuration.color_space.into(),
            width: configuration.width,
            height: configuration.height,
            present_mode: configuration.present_mode.into(),
            desired_maximum_frame_latency: configuration.desired_maximum_frame_latency,
            alpha_mode: configuration.alpha_mode.into(),
            view_formats: configuration
                .view_formats
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<&wgt::SurfaceConfiguration<Vec<wgt::TextureFormat>>> for types::SurfaceConfiguration {
    fn from(configuration: &wgt::SurfaceConfiguration<Vec<wgt::TextureFormat>>) -> Self {
        types::SurfaceConfiguration {
            usage: configuration.usage.into(),
            format: configuration.format.into(),
            color_space: configuration.color_space.into(),
            width: configuration.width,
            height: configuration.height,
            present_mode: configuration.present_mode.into(),
            desired_maximum_frame_latency: configuration.desired_maximum_frame_latency,
            alpha_mode: configuration.alpha_mode.into(),
            view_formats: configuration
                .view_formats
                .iter()
                .map(|&format| format.into())
                .collect(),
        }
    }
}

impl From<wgt::SurfaceCapabilities> for types::SurfaceCapabilities {
    fn from(capabilities: wgt::SurfaceCapabilities) -> Self {
        types::SurfaceCapabilities {
            formats: capabilities.formats.into_iter().map(Into::into).collect(),
            format_capabilities: capabilities
                .format_capabilities
                .into_iter()
                .map(|format| types::SurfaceFormatCapabilities {
                    format: format.format.into(),
                    color_spaces: color_spaces(format.color_spaces),
                })
                .collect(),
            present_modes: capabilities
                .present_modes
                .into_iter()
                .map(Into::into)
                .collect(),
            alpha_modes: capabilities
                .alpha_modes
                .into_iter()
                .map(Into::into)
                .collect(),
            usages: capabilities.usages.into(),
        }
    }
}

/// Each colour space of `spaces`, in the order the package lists them.
fn color_spaces(spaces: wgt::SurfaceColorSpaces) -> Vec<types::SurfaceColorSpace> {
    use wgt::SurfaceColorSpace as Space;
    [
        Space::Srgb,
        Space::ExtendedSrgbLinear,
        Space::DisplayP3,
        Space::Bt2100Pq,
        Space::Bt2100Hlg,
        Space::ExtendedSrgb,
        Space::ExtendedDisplayP3,
    ]
    .into_iter()
    .filter(|space| {
        space
            .to_color_spaces()
            .is_some_and(|flag| spaces.contains(flag))
    })
    .map(Into::into)
    .collect()
}

impl From<wgt::DisplayHdrInfo> for types::DisplayHdrInfo {
    fn from(info: wgt::DisplayHdrInfo) -> Self {
        let pair = |xy: Option<[f32; 2]>| xy.map(|[x, y]| (x, y));
        types::DisplayHdrInfo {
            luminance: info.luminance.map(|luminance| types::DisplayLuminance {
                max_nits: luminance.max_nits,
                max_full_frame_nits: luminance.max_full_frame_nits,
                min_nits: luminance.min_nits,
                sdr_white_nits: luminance.sdr_white_nits,
            }),
            headroom: info.headroom.map(|headroom| types::DisplayHeadroom {
                current: headroom.current,
                potential: headroom.potential,
                reference: headroom.reference,
            }),
            chromaticity: info
                .chromaticity
                .map(|chromaticity| types::DisplayChromaticity {
                    red: pair(chromaticity.red),
                    green: pair(chromaticity.green),
                    blue: pair(chromaticity.blue),
                    white: pair(chromaticity.white),
                }),
            coarse: info.coarse.map(|coarse| types::DisplayCoarseRange {
                high_dynamic_range: coarse.high_dynamic_range,
                gamut: coarse.gamut.and_then(display_gamut),
            }),
            bits_per_color: info.bits_per_color,
        }
    }
}

impl From<crate::DeviceLostInfo> for types::DeviceLostInfo {
    fn from(info: crate::DeviceLostInfo) -> Self {
        types::DeviceLostInfo {
            reason: info.reason.into(),
            message: info.message,
        }
    }
}
