//! What an adapter and a device have, and what a device asks for: features, limits, adapter
//! information, downlevel capabilities, WGSL's language extensions, memory hints and the adapter's
//! presentation clock.

use wasdk::gpu_types as wit;

use super::{enum_conversions, flag_conversions};

/// Each feature of the WIT and wgpu's bit for it. The match in `feature_from_wit` is exhaustive,
/// and the list in `FEATURES` is the same rows, so the WIT's cases and these rows stay one set.
macro_rules! feature_table {
    ($($wit:ident => $wgpu:ident),* $(,)?) => {
        /// wgpu's bit for `feature`.
        pub fn feature_from_wit(feature: wit::Feature) -> wgpu::Features {
            match feature {
                $(wit::Feature::$wit => wgpu::Features::$wgpu,)*
            }
        }

        /// Every feature the WIT names, with wgpu's bit.
        const FEATURES: &[(wit::Feature, wgpu::Features)] = &[
            $((wit::Feature::$wit, wgpu::Features::$wgpu),)*
        ];
    };
}

feature_table! {
    DepthClipControl => DEPTH_CLIP_CONTROL,
    Depth32floatStencil8 => DEPTH32FLOAT_STENCIL8,
    TextureCompressionBc => TEXTURE_COMPRESSION_BC,
    TextureCompressionBcSliced3d => TEXTURE_COMPRESSION_BC_SLICED_3D,
    TextureCompressionEtc2 => TEXTURE_COMPRESSION_ETC2,
    TextureCompressionAstc => TEXTURE_COMPRESSION_ASTC,
    TextureCompressionAstcSliced3d => TEXTURE_COMPRESSION_ASTC_SLICED_3D,
    TimestampQuery => TIMESTAMP_QUERY,
    IndirectFirstInstance => INDIRECT_FIRST_INSTANCE,
    ShaderF16 => SHADER_F16,
    Rg11b10ufloatRenderable => RG11B10UFLOAT_RENDERABLE,
    Bgra8unormStorage => BGRA8UNORM_STORAGE,
    Float32Filterable => FLOAT32_FILTERABLE,
    Float32Blendable => FLOAT32_BLENDABLE,
    DualSourceBlending => DUAL_SOURCE_BLENDING,
    ClipDistances => CLIP_DISTANCES,
    Immediates => IMMEDIATES,
    PrimitiveIndex => PRIMITIVE_INDEX,
    ShaderFloat32Atomic => SHADER_FLOAT32_ATOMIC,
    TextureFormat16bitNorm => TEXTURE_FORMAT_16BIT_NORM,
    TextureCompressionAstcHdr => TEXTURE_COMPRESSION_ASTC_HDR,
    TextureAdapterSpecificFormatFeatures => TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    PipelineStatisticsQuery => PIPELINE_STATISTICS_QUERY,
    TimestampQueryInsideEncoders => TIMESTAMP_QUERY_INSIDE_ENCODERS,
    TimestampQueryInsidePasses => TIMESTAMP_QUERY_INSIDE_PASSES,
    MappablePrimaryBuffers => MAPPABLE_PRIMARY_BUFFERS,
    TextureBindingArray => TEXTURE_BINDING_ARRAY,
    BufferBindingArray => BUFFER_BINDING_ARRAY,
    StorageResourceBindingArray => STORAGE_RESOURCE_BINDING_ARRAY,
    SampledTextureAndStorageBufferArrayNonUniformIndexing =>
        SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    StorageTextureArrayNonUniformIndexing => STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING,
    PartiallyBoundBindingArray => PARTIALLY_BOUND_BINDING_ARRAY,
    MultiDrawIndirectCount => MULTI_DRAW_INDIRECT_COUNT,
    AddressModeClampToZero => ADDRESS_MODE_CLAMP_TO_ZERO,
    AddressModeClampToBorder => ADDRESS_MODE_CLAMP_TO_BORDER,
    PolygonModeLine => POLYGON_MODE_LINE,
    PolygonModePoint => POLYGON_MODE_POINT,
    ConservativeRasterization => CONSERVATIVE_RASTERIZATION,
    VertexWritableStorage => VERTEX_WRITABLE_STORAGE,
    ClearTexture => CLEAR_TEXTURE,
    Multiview => MULTIVIEW,
    VertexAttribute64bit => VERTEX_ATTRIBUTE_64BIT,
    TextureAtomic => TEXTURE_ATOMIC,
    TextureFormatNv12 => TEXTURE_FORMAT_NV12,
    TextureFormatP010 => TEXTURE_FORMAT_P010,
    ExternalTexture => EXTERNAL_TEXTURE,
    ShaderF64 => SHADER_F64,
    ShaderI16 => SHADER_I16,
    ShaderEarlyDepthTest => SHADER_EARLY_DEPTH_TEST,
    ShaderInt64 => SHADER_INT64,
    Subgroup => SUBGROUP,
    SubgroupVertex => SUBGROUP_VERTEX,
    SubgroupBarrier => SUBGROUP_BARRIER,
    ShaderInt64AtomicMinMax => SHADER_INT64_ATOMIC_MIN_MAX,
    ShaderInt64AtomicAllOps => SHADER_INT64_ATOMIC_ALL_OPS,
    TextureInt64Atomic => TEXTURE_INT64_ATOMIC,
    UniformBufferBindingArrays => UNIFORM_BUFFER_BINDING_ARRAYS,
    ShaderBarycentrics => SHADER_BARYCENTRICS,
    SelectiveMultiview => SELECTIVE_MULTIVIEW,
    MultisampleArray => MULTISAMPLE_ARRAY,
    ShaderPerVertex => SHADER_PER_VERTEX,
    ShaderDrawIndex => SHADER_DRAW_INDEX,
    MemoryDecorationCoherent => MEMORY_DECORATION_COHERENT,
    MemoryDecorationVolatile => MEMORY_DECORATION_VOLATILE,
}

pub fn features_from_wit(features: Vec<wit::Feature>) -> wgpu::Features {
    (features.into_iter())
        .map(feature_from_wit)
        .fold(wgpu::Features::empty(), |all, feature| all | feature)
}

/// The WIT's features of `features`, or those the WIT leaves out.
pub fn features_to_wit(features: wgpu::Features) -> Result<Vec<wit::Feature>, wgpu::Features> {
    let mut rest = features;
    let named = (FEATURES.iter())
        .filter(|(_, bit)| features.contains(*bit))
        .map(|&(feature, bit)| {
            rest.remove(bit);
            feature
        })
        .collect();
    if rest.is_empty() {
        Ok(named)
    } else {
        Err(rest)
    }
}

/// Every limit of the WIT, with wgpu's field and the WIT's getter and setter; and wgpu's fields
/// the WIT leaves out, which `limits_to_wit` names so that a field wgpu adds fails to build.
macro_rules! limits_table {
    (
        $($field:ident: $getter:ident / $setter:ident),* $(,)?;
        left out: $($left_out:ident),* $(,)?
    ) => {
        /// wgpu's limits of a set the engine answered; the limits the WIT leaves out keep wgpu's
        /// defaults.
        pub fn limits_from_wit(limits: &wit::Limits) -> wgpu::Limits {
            wgpu::Limits {
                $($field: limits.$getter(),)*
                ..wgpu::Limits::default()
            }
        }

        /// A set of the WIT's limits holding `limits`; those the WIT leaves out, which only the
        /// features it leaves out raise, are not asked for.
        pub fn limits_to_wit(limits: &wgpu::Limits) -> wit::Limits {
            let &wgpu::Limits { $($field,)* $($left_out: _,)* } = limits;
            let wit = wit::Limits::new();
            $(wit.$setter($field);)*
            wit
        }
    };
}

limits_table! {
    max_texture_dimension_1d: max_texture_dimension1d / set_max_texture_dimension1d,
    max_texture_dimension_2d: max_texture_dimension2d / set_max_texture_dimension2d,
    max_texture_dimension_3d: max_texture_dimension3d / set_max_texture_dimension3d,
    max_texture_array_layers: max_texture_array_layers / set_max_texture_array_layers,
    max_bind_groups: max_bind_groups / set_max_bind_groups,
    max_bind_groups_plus_vertex_buffers:
        max_bind_groups_plus_vertex_buffers / set_max_bind_groups_plus_vertex_buffers,
    max_bindings_per_bind_group: max_bindings_per_bind_group / set_max_bindings_per_bind_group,
    max_dynamic_uniform_buffers_per_pipeline_layout:
        max_dynamic_uniform_buffers_per_pipeline_layout
            / set_max_dynamic_uniform_buffers_per_pipeline_layout,
    max_dynamic_storage_buffers_per_pipeline_layout:
        max_dynamic_storage_buffers_per_pipeline_layout
            / set_max_dynamic_storage_buffers_per_pipeline_layout,
    max_sampled_textures_per_shader_stage:
        max_sampled_textures_per_shader_stage / set_max_sampled_textures_per_shader_stage,
    max_samplers_per_shader_stage:
        max_samplers_per_shader_stage / set_max_samplers_per_shader_stage,
    max_storage_buffers_per_shader_stage:
        max_storage_buffers_per_shader_stage / set_max_storage_buffers_per_shader_stage,
    max_storage_textures_per_shader_stage:
        max_storage_textures_per_shader_stage / set_max_storage_textures_per_shader_stage,
    max_uniform_buffers_per_shader_stage:
        max_uniform_buffers_per_shader_stage / set_max_uniform_buffers_per_shader_stage,
    max_binding_array_elements_per_shader_stage:
        max_binding_array_elements_per_shader_stage
            / set_max_binding_array_elements_per_shader_stage,
    max_binding_array_sampler_elements_per_shader_stage:
        max_binding_array_sampler_elements_per_shader_stage
            / set_max_binding_array_sampler_elements_per_shader_stage,
    max_uniform_buffer_binding_size:
        max_uniform_buffer_binding_size / set_max_uniform_buffer_binding_size,
    max_storage_buffer_binding_size:
        max_storage_buffer_binding_size / set_max_storage_buffer_binding_size,
    max_vertex_buffers: max_vertex_buffers / set_max_vertex_buffers,
    max_buffer_size: max_buffer_size / set_max_buffer_size,
    max_vertex_attributes: max_vertex_attributes / set_max_vertex_attributes,
    max_vertex_buffer_array_stride:
        max_vertex_buffer_array_stride / set_max_vertex_buffer_array_stride,
    max_inter_stage_shader_variables:
        max_inter_stage_shader_variables / set_max_inter_stage_shader_variables,
    min_uniform_buffer_offset_alignment:
        min_uniform_buffer_offset_alignment / set_min_uniform_buffer_offset_alignment,
    min_storage_buffer_offset_alignment:
        min_storage_buffer_offset_alignment / set_min_storage_buffer_offset_alignment,
    max_color_attachments: max_color_attachments / set_max_color_attachments,
    max_color_attachment_bytes_per_sample:
        max_color_attachment_bytes_per_sample / set_max_color_attachment_bytes_per_sample,
    max_compute_workgroup_storage_size:
        max_compute_workgroup_storage_size / set_max_compute_workgroup_storage_size,
    max_compute_invocations_per_workgroup:
        max_compute_invocations_per_workgroup / set_max_compute_invocations_per_workgroup,
    max_compute_workgroup_size_x: max_compute_workgroup_size_x / set_max_compute_workgroup_size_x,
    max_compute_workgroup_size_y: max_compute_workgroup_size_y / set_max_compute_workgroup_size_y,
    max_compute_workgroup_size_z: max_compute_workgroup_size_z / set_max_compute_workgroup_size_z,
    max_compute_workgroups_per_dimension:
        max_compute_workgroups_per_dimension / set_max_compute_workgroups_per_dimension,
    max_immediate_size: max_immediate_size / set_max_immediate_size,
    max_non_sampler_bindings: max_non_sampler_bindings / set_max_non_sampler_bindings,
    max_buffers_and_acceleration_structures_per_shader_stage:
        max_buffers_and_acceleration_structures_per_shader_stage
            / set_max_buffers_and_acceleration_structures_per_shader_stage,
    max_multiview_view_count: max_multiview_view_count / set_max_multiview_view_count;
    left out:
    max_binding_array_acceleration_structure_elements_per_shader_stage,
    max_task_workgroup_total_count,
    max_task_workgroups_per_dimension,
    max_mesh_workgroup_total_count,
    max_mesh_workgroups_per_dimension,
    max_task_invocations_per_workgroup,
    max_task_invocations_per_dimension,
    max_mesh_invocations_per_workgroup,
    max_mesh_invocations_per_dimension,
    max_task_payload_size,
    max_mesh_output_vertices,
    max_mesh_output_primitives,
    max_mesh_output_layers,
    max_mesh_multiview_view_count,
    max_blas_primitive_count,
    max_blas_geometry_count,
    max_tlas_instance_count,
    max_acceleration_structures_per_shader_stage,
    max_ray_dispatch_count,
    max_ray_recursion_depth,
}

enum_conversions! {
    power_preference_to_wit, power_preference_from_wit: PowerPreference <=> PowerPreference {
        None, LowPower, HighPerformance,
    }
}

enum_conversions! {
    backend_to_wit, backend_from_wit: Backend <=> Backend {
        Noop, Vulkan, Metal, Dx12, Gl, BrowserWebGpu,
    }
}

enum_conversions! {
    device_type_to_wit, device_type_from_wit: DeviceType <=> DeviceType {
        Other, IntegratedGpu, DiscreteGpu, VirtualGpu, Cpu,
    }
}

enum_conversions! {
    shader_model_to_wit, shader_model_from_wit: ShaderModel <=> ShaderModel {
        Sm2, Sm4, Sm5,
    }
}

flag_conversions! {
    downlevel_flags_to_wit, downlevel_flags_from_wit: DownlevelFlags <=> DownlevelFlags {
        COMPUTE_SHADERS, FRAGMENT_WRITABLE_STORAGE, INDIRECT_EXECUTION, BASE_VERTEX,
        READ_ONLY_DEPTH_STENCIL, NON_POWER_OF_TWO_MIPMAPPED_TEXTURES, CUBE_ARRAY_TEXTURES,
        COMPARISON_SAMPLERS, INDEPENDENT_BLEND, VERTEX_STORAGE, ANISOTROPIC_FILTERING,
        FRAGMENT_STORAGE, MULTISAMPLED_SHADING, DEPTH_TEXTURE_AND_BUFFER_COPIES,
        WEBGPU_TEXTURE_FORMAT_SUPPORT,
        BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED => BUFFER_BINDINGS_NOT16_BYTE_ALIGNED,
        UNRESTRICTED_INDEX_BUFFER, FULL_DRAW_INDEX_UINT32,
    }
}

pub fn downlevel_capabilities_from_wit(
    capabilities: wit::DownlevelCapabilities,
) -> wgpu::DownlevelCapabilities {
    wgpu::DownlevelCapabilities {
        flags: downlevel_flags_from_wit(capabilities.flags),
        limits: wgpu::DownlevelLimits::default(),
        shader_model: shader_model_from_wit(capabilities.shader_model),
    }
}

/// What the adapter says of itself; the limit bucket, the engine's policy, is not told.
pub fn adapter_info_from_wit(info: wit::AdapterInfo) -> wgpu::AdapterInfo {
    wgpu::AdapterInfo {
        name: info.name,
        vendor: info.vendor,
        device: info.device,
        device_type: device_type_from_wit(info.device_type),
        device_pci_bus_id: info.device_pci_bus_id,
        driver: info.driver,
        driver_info: info.driver_info,
        backend: backend_from_wit(info.backend),
        subgroup_min_size: info.subgroup_min_size,
        subgroup_max_size: info.subgroup_max_size,
        transient_saves_memory: info.transient_saves_memory,
        limit_bucket: None,
    }
}

pub fn texture_format_features_from_wit(
    features: wit::TextureFormatFeatures,
) -> wgpu::TextureFormatFeatures {
    wgpu::TextureFormatFeatures {
        allowed_usages: super::texture_usages_from_wit(features.allowed_usages),
        flags: super::texture_format_feature_flags_from_wit(features.flags),
    }
}

pub fn wgsl_language_features_from_wit(
    extensions: wit::WgslLanguageExtensions,
) -> wgpu::WgslLanguageFeatures {
    use wgpu::WgslLanguageFeatures as Wgpu;
    use wit::WgslLanguageExtensions as Wit;
    [
        (
            Wit::READ_ONLY_AND_READ_WRITE_STORAGE_TEXTURES,
            Wgpu::ReadOnlyAndReadWriteStorageTextures,
        ),
        (
            Wit::PACKED4X8_INTEGER_DOT_PRODUCT,
            Wgpu::Packed4x8IntegerDotProduct,
        ),
        (
            Wit::UNRESTRICTED_POINTER_PARAMETERS,
            Wgpu::UnrestrictedPointerParameters,
        ),
        (Wit::POINTER_COMPOSITE_ACCESS, Wgpu::PointerCompositeAccess),
    ]
    .into_iter()
    .filter(|(extension, _)| extensions.contains(*extension))
    .fold(Wgpu::empty(), |all, (_, feature)| all | feature)
}

pub fn memory_hints_to_wit(hints: &wgpu::MemoryHints) -> wit::MemoryHints {
    match hints {
        wgpu::MemoryHints::Performance => wit::MemoryHints::Performance,
        wgpu::MemoryHints::MemoryUsage => wit::MemoryHints::MemoryUsage,
        wgpu::MemoryHints::Manual {
            suballocated_device_memory_block_size,
        } => wit::MemoryHints::Manual((
            suballocated_device_memory_block_size.start,
            suballocated_device_memory_block_size.end,
        )),
    }
}

/// The adapter's clock as the WIT answers it, none being wgpu's invalid timestamp.
pub fn presentation_timestamp_from_wit(timestamp: Option<u64>) -> wgpu::PresentationTimestamp {
    timestamp.map_or(wgpu::PresentationTimestamp::INVALID_TIMESTAMP, |ticks| {
        wgpu::PresentationTimestamp(u128::from(ticks))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_feature_comes_back_as_itself() {
        let all = (FEATURES.iter()).fold(wgpu::Features::empty(), |all, (_, bit)| all | *bit);
        let named = features_to_wit(all).expect("every row is the WIT's");
        assert_eq!(named.len(), FEATURES.len());
        assert_eq!(features_from_wit(named), all);
    }

    #[test]
    fn a_feature_the_wit_leaves_out_is_reported() {
        let asked = wgpu::Features::SHADER_F16 | wgpu::Features::EXPERIMENTAL_RAY_QUERY;
        assert_eq!(
            features_to_wit(asked),
            Err(wgpu::Features::EXPERIMENTAL_RAY_QUERY)
        );
    }
}
