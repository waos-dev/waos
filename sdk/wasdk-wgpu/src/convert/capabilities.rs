//! Features, limits, adapter information and errors: what the device reports and asks for.

use wasdk::webgpu::{
    GpuAdapterInfo, GpuCompilationInfo, GpuCompilationMessage, GpuCompilationMessageType,
    GpuDeviceLostReason, GpuError, GpuErrorFilter, GpuErrorKind, GpuFeatureName,
    GpuPowerPreference, GpuSupportedFeatures, GpuSupportedLimits, RecordOptionGpuSize64,
};
use wgpu::{
    AdapterInfo, Backend, CompilationInfo, CompilationMessage, CompilationMessageType,
    DeviceLostReason, DeviceType, Error, ErrorFilter, ErrorSource, Features, Limits,
    PowerPreference, SourceLocation,
};

/// The WebGPU features, by wgpu bit, specification name (what `has` is asked) and WIT name.
/// wasi features without a wgpu bit (`core-features-and-limits`, `texture-formats-tier1/2`,
/// `texture-component-swizzle`) have nothing to be reported as.
const FEATURES: &[(Features, &str, GpuFeatureName)] = &[
    (
        Features::DEPTH_CLIP_CONTROL,
        "depth-clip-control",
        GpuFeatureName::DepthClipControl,
    ),
    (
        Features::DEPTH32FLOAT_STENCIL8,
        "depth32float-stencil8",
        GpuFeatureName::Depth32floatStencil8,
    ),
    (
        Features::TEXTURE_COMPRESSION_BC,
        "texture-compression-bc",
        GpuFeatureName::TextureCompressionBc,
    ),
    (
        Features::TEXTURE_COMPRESSION_BC_SLICED_3D,
        "texture-compression-bc-sliced-3d",
        GpuFeatureName::TextureCompressionBcSliced3d,
    ),
    (
        Features::TEXTURE_COMPRESSION_ETC2,
        "texture-compression-etc2",
        GpuFeatureName::TextureCompressionEtc2,
    ),
    (
        Features::TEXTURE_COMPRESSION_ASTC,
        "texture-compression-astc",
        GpuFeatureName::TextureCompressionAstc,
    ),
    (
        Features::TEXTURE_COMPRESSION_ASTC_SLICED_3D,
        "texture-compression-astc-sliced-3d",
        GpuFeatureName::TextureCompressionAstcSliced3d,
    ),
    (
        Features::TIMESTAMP_QUERY,
        "timestamp-query",
        GpuFeatureName::TimestampQuery,
    ),
    (
        Features::INDIRECT_FIRST_INSTANCE,
        "indirect-first-instance",
        GpuFeatureName::IndirectFirstInstance,
    ),
    (
        Features::SHADER_F16,
        "shader-f16",
        GpuFeatureName::ShaderF16,
    ),
    (
        Features::RG11B10UFLOAT_RENDERABLE,
        "rg11b10ufloat-renderable",
        GpuFeatureName::Rg11b10ufloatRenderable,
    ),
    (
        Features::BGRA8UNORM_STORAGE,
        "bgra8unorm-storage",
        GpuFeatureName::Bgra8unormStorage,
    ),
    (
        Features::FLOAT32_FILTERABLE,
        "float32-filterable",
        GpuFeatureName::Float32Filterable,
    ),
    (
        Features::FLOAT32_BLENDABLE,
        "float32-blendable",
        GpuFeatureName::Float32Blendable,
    ),
    (
        Features::CLIP_DISTANCES,
        "clip-distances",
        GpuFeatureName::ClipDistances,
    ),
    (
        Features::DUAL_SOURCE_BLENDING,
        "dual-source-blending",
        GpuFeatureName::DualSourceBlending,
    ),
    (Features::SUBGROUP, "subgroups", GpuFeatureName::Subgroups),
    (
        Features::PRIMITIVE_INDEX,
        "primitive-index",
        GpuFeatureName::PrimitiveIndex,
    ),
];

pub fn features_from_wasi(supported: &GpuSupportedFeatures) -> Features {
    FEATURES
        .iter()
        .filter(|(_, name, _)| supported.has(name))
        .fold(Features::empty(), |features, (bit, ..)| features | *bit)
}

/// The WIT names of `required`, or the features wasi:webgpu has no name for.
pub fn features_to_wasi(required: Features) -> Result<Vec<GpuFeatureName>, Features> {
    let mut names = Vec::new();
    let mut unsupported = required;
    for (bit, _, name) in FEATURES {
        unsupported.remove(*bit);
        if required.contains(*bit) {
            names.push(*name);
        }
    }
    if unsupported.is_empty() {
        Ok(names)
    } else {
        Err(unsupported)
    }
}

/// wgpu limits with no WebGPU counterpart (binding arrays, mesh shaders, ray tracing,
/// multiview) stay at wgpu's defaults.
pub fn limits_from_wasi(limits: &GpuSupportedLimits) -> Limits {
    Limits {
        max_texture_dimension_1d: limits.max_texture_dimension1_d(),
        max_texture_dimension_2d: limits.max_texture_dimension2_d(),
        max_texture_dimension_3d: limits.max_texture_dimension3_d(),
        max_texture_array_layers: limits.max_texture_array_layers(),
        max_bind_groups: limits.max_bind_groups(),
        max_bind_groups_plus_vertex_buffers: limits.max_bind_groups_plus_vertex_buffers(),
        max_bindings_per_bind_group: limits.max_bindings_per_bind_group(),
        max_dynamic_uniform_buffers_per_pipeline_layout: limits
            .max_dynamic_uniform_buffers_per_pipeline_layout(),
        max_dynamic_storage_buffers_per_pipeline_layout: limits
            .max_dynamic_storage_buffers_per_pipeline_layout(),
        max_sampled_textures_per_shader_stage: limits.max_sampled_textures_per_shader_stage(),
        max_samplers_per_shader_stage: limits.max_samplers_per_shader_stage(),
        max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage(),
        max_storage_textures_per_shader_stage: limits.max_storage_textures_per_shader_stage(),
        max_uniform_buffers_per_shader_stage: limits.max_uniform_buffers_per_shader_stage(),
        max_uniform_buffer_binding_size: limits.max_uniform_buffer_binding_size(),
        max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size(),
        max_vertex_buffers: limits.max_vertex_buffers(),
        max_buffer_size: limits.max_buffer_size(),
        max_vertex_attributes: limits.max_vertex_attributes(),
        max_vertex_buffer_array_stride: limits.max_vertex_buffer_array_stride(),
        max_inter_stage_shader_variables: limits.max_inter_stage_shader_variables(),
        min_uniform_buffer_offset_alignment: limits.min_uniform_buffer_offset_alignment(),
        min_storage_buffer_offset_alignment: limits.min_storage_buffer_offset_alignment(),
        max_color_attachments: limits.max_color_attachments(),
        max_color_attachment_bytes_per_sample: limits.max_color_attachment_bytes_per_sample(),
        max_compute_workgroup_storage_size: limits.max_compute_workgroup_storage_size(),
        max_compute_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup(),
        max_compute_workgroup_size_x: limits.max_compute_workgroup_size_x(),
        max_compute_workgroup_size_y: limits.max_compute_workgroup_size_y(),
        max_compute_workgroup_size_z: limits.max_compute_workgroup_size_z(),
        max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension(),
        max_immediate_size: limits.max_immediate_size(),
        ..Limits::default()
    }
}

/// The requested limits under their `GPUSupportedLimits` member names. Limits wgpu has and
/// WebGPU does not are not requestable and are left out.
pub fn limits_to_wasi(limits: &Limits) -> RecordOptionGpuSize64 {
    let record = RecordOptionGpuSize64::new();
    let entries: [(&str, u64); 32] = [
        (
            "maxTextureDimension1D",
            limits.max_texture_dimension_1d.into(),
        ),
        (
            "maxTextureDimension2D",
            limits.max_texture_dimension_2d.into(),
        ),
        (
            "maxTextureDimension3D",
            limits.max_texture_dimension_3d.into(),
        ),
        (
            "maxTextureArrayLayers",
            limits.max_texture_array_layers.into(),
        ),
        ("maxBindGroups", limits.max_bind_groups.into()),
        (
            "maxBindGroupsPlusVertexBuffers",
            limits.max_bind_groups_plus_vertex_buffers.into(),
        ),
        (
            "maxBindingsPerBindGroup",
            limits.max_bindings_per_bind_group.into(),
        ),
        (
            "maxDynamicUniformBuffersPerPipelineLayout",
            limits
                .max_dynamic_uniform_buffers_per_pipeline_layout
                .into(),
        ),
        (
            "maxDynamicStorageBuffersPerPipelineLayout",
            limits
                .max_dynamic_storage_buffers_per_pipeline_layout
                .into(),
        ),
        (
            "maxSampledTexturesPerShaderStage",
            limits.max_sampled_textures_per_shader_stage.into(),
        ),
        (
            "maxSamplersPerShaderStage",
            limits.max_samplers_per_shader_stage.into(),
        ),
        (
            "maxStorageBuffersPerShaderStage",
            limits.max_storage_buffers_per_shader_stage.into(),
        ),
        (
            "maxStorageTexturesPerShaderStage",
            limits.max_storage_textures_per_shader_stage.into(),
        ),
        (
            "maxUniformBuffersPerShaderStage",
            limits.max_uniform_buffers_per_shader_stage.into(),
        ),
        (
            "maxUniformBufferBindingSize",
            limits.max_uniform_buffer_binding_size,
        ),
        (
            "maxStorageBufferBindingSize",
            limits.max_storage_buffer_binding_size,
        ),
        (
            "minUniformBufferOffsetAlignment",
            limits.min_uniform_buffer_offset_alignment.into(),
        ),
        (
            "minStorageBufferOffsetAlignment",
            limits.min_storage_buffer_offset_alignment.into(),
        ),
        ("maxVertexBuffers", limits.max_vertex_buffers.into()),
        ("maxBufferSize", limits.max_buffer_size),
        ("maxVertexAttributes", limits.max_vertex_attributes.into()),
        (
            "maxVertexBufferArrayStride",
            limits.max_vertex_buffer_array_stride.into(),
        ),
        (
            "maxInterStageShaderVariables",
            limits.max_inter_stage_shader_variables.into(),
        ),
        ("maxColorAttachments", limits.max_color_attachments.into()),
        (
            "maxColorAttachmentBytesPerSample",
            limits.max_color_attachment_bytes_per_sample.into(),
        ),
        (
            "maxComputeWorkgroupStorageSize",
            limits.max_compute_workgroup_storage_size.into(),
        ),
        (
            "maxComputeInvocationsPerWorkgroup",
            limits.max_compute_invocations_per_workgroup.into(),
        ),
        (
            "maxComputeWorkgroupSizeX",
            limits.max_compute_workgroup_size_x.into(),
        ),
        (
            "maxComputeWorkgroupSizeY",
            limits.max_compute_workgroup_size_y.into(),
        ),
        (
            "maxComputeWorkgroupSizeZ",
            limits.max_compute_workgroup_size_z.into(),
        ),
        (
            "maxComputeWorkgroupsPerDimension",
            limits.max_compute_workgroups_per_dimension.into(),
        ),
        ("maxImmediateSize", limits.max_immediate_size.into()),
    ];
    for (key, value) in entries {
        record.add(key, Some(value));
    }
    record
}

/// The wasi info is strings where wgpu has PCI ids: the description becomes the name and the
/// rest lands in `driver_info`. The backend is reported as WebGPU, the API this is.
pub fn adapter_info_from_wasi(info: &GpuAdapterInfo) -> AdapterInfo {
    AdapterInfo {
        name: info.description(),
        vendor: 0,
        device: 0,
        device_type: if info.is_fallback_adapter() {
            DeviceType::Cpu
        } else {
            DeviceType::Other
        },
        device_pci_bus_id: String::new(),
        driver: String::new(),
        driver_info: format!(
            "vendor={} architecture={} device={}",
            info.vendor(),
            info.architecture(),
            info.device()
        ),
        backend: Backend::BrowserWebGpu,
        subgroup_min_size: info.subgroup_min_size(),
        subgroup_max_size: info.subgroup_max_size(),
        transient_saves_memory: None,
        limit_bucket: None,
    }
}

pub fn power_preference_to_wasi(preference: PowerPreference) -> Option<GpuPowerPreference> {
    match preference {
        PowerPreference::None => None,
        PowerPreference::LowPower => Some(GpuPowerPreference::LowPower),
        PowerPreference::HighPerformance => Some(GpuPowerPreference::HighPerformance),
    }
}

pub fn error_filter_to_wasi(filter: ErrorFilter) -> GpuErrorFilter {
    match filter {
        ErrorFilter::Validation => GpuErrorFilter::Validation,
        ErrorFilter::OutOfMemory => GpuErrorFilter::OutOfMemory,
        ErrorFilter::Internal => GpuErrorFilter::Internal,
    }
}

pub fn error_from_wasi(error: &GpuError) -> Error {
    let source: ErrorSource = Box::from("wasi:webgpu");
    match error.kind() {
        GpuErrorKind::ValidationError => Error::Validation {
            source,
            description: error.message(),
        },
        GpuErrorKind::OutOfMemoryError => Error::OutOfMemory { source },
        GpuErrorKind::InternalError => Error::Internal {
            source,
            description: error.message(),
        },
    }
}

pub fn device_lost_reason_from_wasi(reason: GpuDeviceLostReason) -> DeviceLostReason {
    match reason {
        GpuDeviceLostReason::Unknown => DeviceLostReason::Unknown,
        GpuDeviceLostReason::Destroyed => DeviceLostReason::Destroyed,
    }
}

pub fn compilation_info_from_wasi(info: &GpuCompilationInfo) -> CompilationInfo {
    CompilationInfo {
        messages: info
            .messages()
            .iter()
            .map(compilation_message_from_wasi)
            .collect(),
    }
}

/// Locations are passed through as the host reports them; a message without a span (offset
/// and length both zero) has none.
pub fn compilation_message_from_wasi(message: &GpuCompilationMessage) -> CompilationMessage {
    let message_type = match message.type_() {
        GpuCompilationMessageType::Error => CompilationMessageType::Error,
        GpuCompilationMessageType::Warning => CompilationMessageType::Warning,
        GpuCompilationMessageType::Info => CompilationMessageType::Info,
    };
    let (offset, length) = (message.offset(), message.length());
    let location = (offset != 0 || length != 0).then(|| SourceLocation {
        line_number: message.line_num() as u32,
        line_position: message.line_pos() as u32,
        offset: offset as u32,
        length: length as u32,
    });
    CompilationMessage {
        message: message.message(),
        message_type,
        location,
    }
}
