//! The features: each case of the package's `feature` and wgpu-types' constant for it, one table
//! read both ways. wgpu's features the package leaves out are listed by name, with the reason in
//! the package's own docs, so a test can tell a feature a later wgpu adds from one decided on.

use wgpu_types::Features;

use crate::wasdk::gpu::types::Feature;

macro_rules! features {
    ($($case:ident = $constant:ident,)*) => {
        /// Each feature with its constant.
        const TABLE: &[(Feature, Features)] = &[$((Feature::$case, Features::$constant),)*];

        /// `feature` as wgpu-types' flag.
        fn flag(feature: Feature) -> Features {
            match feature {
                $(Feature::$case => Features::$constant,)*
            }
        }
    };
}

features! {
    DepthClipControl = DEPTH_CLIP_CONTROL,
    Depth32floatStencil8 = DEPTH32FLOAT_STENCIL8,
    TextureCompressionBc = TEXTURE_COMPRESSION_BC,
    TextureCompressionBcSliced3d = TEXTURE_COMPRESSION_BC_SLICED_3D,
    TextureCompressionEtc2 = TEXTURE_COMPRESSION_ETC2,
    TextureCompressionAstc = TEXTURE_COMPRESSION_ASTC,
    TextureCompressionAstcSliced3d = TEXTURE_COMPRESSION_ASTC_SLICED_3D,
    TimestampQuery = TIMESTAMP_QUERY,
    IndirectFirstInstance = INDIRECT_FIRST_INSTANCE,
    ShaderF16 = SHADER_F16,
    Rg11b10ufloatRenderable = RG11B10UFLOAT_RENDERABLE,
    Bgra8unormStorage = BGRA8UNORM_STORAGE,
    Float32Filterable = FLOAT32_FILTERABLE,
    Float32Blendable = FLOAT32_BLENDABLE,
    DualSourceBlending = DUAL_SOURCE_BLENDING,
    ClipDistances = CLIP_DISTANCES,
    Immediates = IMMEDIATES,
    PrimitiveIndex = PRIMITIVE_INDEX,
    ShaderFloat32Atomic = SHADER_FLOAT32_ATOMIC,
    TextureFormat16bitNorm = TEXTURE_FORMAT_16BIT_NORM,
    TextureCompressionAstcHdr = TEXTURE_COMPRESSION_ASTC_HDR,
    TextureAdapterSpecificFormatFeatures = TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    PipelineStatisticsQuery = PIPELINE_STATISTICS_QUERY,
    TimestampQueryInsideEncoders = TIMESTAMP_QUERY_INSIDE_ENCODERS,
    TimestampQueryInsidePasses = TIMESTAMP_QUERY_INSIDE_PASSES,
    MappablePrimaryBuffers = MAPPABLE_PRIMARY_BUFFERS,
    TextureBindingArray = TEXTURE_BINDING_ARRAY,
    BufferBindingArray = BUFFER_BINDING_ARRAY,
    StorageResourceBindingArray = STORAGE_RESOURCE_BINDING_ARRAY,
    SampledTextureAndStorageBufferArrayNonUniformIndexing =
        SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    StorageTextureArrayNonUniformIndexing = STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING,
    PartiallyBoundBindingArray = PARTIALLY_BOUND_BINDING_ARRAY,
    MultiDrawIndirectCount = MULTI_DRAW_INDIRECT_COUNT,
    AddressModeClampToZero = ADDRESS_MODE_CLAMP_TO_ZERO,
    AddressModeClampToBorder = ADDRESS_MODE_CLAMP_TO_BORDER,
    PolygonModeLine = POLYGON_MODE_LINE,
    PolygonModePoint = POLYGON_MODE_POINT,
    ConservativeRasterization = CONSERVATIVE_RASTERIZATION,
    VertexWritableStorage = VERTEX_WRITABLE_STORAGE,
    ClearTexture = CLEAR_TEXTURE,
    Multiview = MULTIVIEW,
    VertexAttribute64bit = VERTEX_ATTRIBUTE_64BIT,
    TextureAtomic = TEXTURE_ATOMIC,
    TextureFormatNv12 = TEXTURE_FORMAT_NV12,
    TextureFormatP010 = TEXTURE_FORMAT_P010,
    ExternalTexture = EXTERNAL_TEXTURE,
    ShaderF64 = SHADER_F64,
    ShaderI16 = SHADER_I16,
    ShaderEarlyDepthTest = SHADER_EARLY_DEPTH_TEST,
    ShaderInt64 = SHADER_INT64,
    Subgroup = SUBGROUP,
    SubgroupVertex = SUBGROUP_VERTEX,
    SubgroupBarrier = SUBGROUP_BARRIER,
    ShaderInt64AtomicMinMax = SHADER_INT64_ATOMIC_MIN_MAX,
    ShaderInt64AtomicAllOps = SHADER_INT64_ATOMIC_ALL_OPS,
    TextureInt64Atomic = TEXTURE_INT64_ATOMIC,
    UniformBufferBindingArrays = UNIFORM_BUFFER_BINDING_ARRAYS,
    ShaderBarycentrics = SHADER_BARYCENTRICS,
    SelectiveMultiview = SELECTIVE_MULTIVIEW,
    MultisampleArray = MULTISAMPLE_ARRAY,
    ShaderPerVertex = SHADER_PER_VERTEX,
    ShaderDrawIndex = SHADER_DRAW_INDEX,
    MemoryDecorationCoherent = MEMORY_DECORATION_COHERENT,
    MemoryDecorationVolatile = MEMORY_DECORATION_VOLATILE,
}

/// The features `features` names, as the flags wgpu-types takes.
pub(crate) fn feature_set(features: &[Feature]) -> Features {
    features
        .iter()
        .fold(Features::empty(), |set, &feature| set | flag(feature))
}

/// The features of `flags` the package names; those it leaves out are not answered.
pub(crate) fn features_of(flags: Features) -> Vec<Feature> {
    TABLE
        .iter()
        .filter(|(_, constant)| flags.contains(*constant))
        .map(|(feature, _)| *feature)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wgpu's features the package leaves out: the experimental ones and those that only serve
    /// them, passthrough shaders, and the engine's own.
    const LEFT_OUT: &[&str] = &[
        "EXPERIMENTAL_RAY_QUERY",
        "EXPERIMENTAL_MESH_SHADER",
        "EXPERIMENTAL_RAY_HIT_VERTEX_RETURN",
        "EXPERIMENTAL_MESH_SHADER_MULTIVIEW",
        "EXPERIMENTAL_MESH_SHADER_POINTS",
        "EXPERIMENTAL_COOPERATIVE_MATRIX",
        "EXPERIMENTAL_RAY_TRACING_PIPELINES",
        "EXTENDED_ACCELERATION_STRUCTURE_VERTEX_FORMATS",
        "ACCELERATION_STRUCTURE_BINDING_ARRAY",
        "PASSTHROUGH_SHADERS",
        "PIPELINE_CACHE",
        "VULKAN_GOOGLE_DISPLAY_TIMING",
        "VULKAN_EXTERNAL_MEMORY_WIN32",
        "VULKAN_EXTERNAL_MEMORY_FD",
        "VULKAN_EXTERNAL_MEMORY_DMA_BUF",
    ];

    #[test]
    fn every_feature_of_wgpus_is_in_the_table_or_left_out() {
        for (name, constant) in bitflags::Flags::iter_names(&Features::all()) {
            let in_table = TABLE.iter().any(|(_, flag)| *flag == constant);
            assert!(
                in_table != LEFT_OUT.contains(&name),
                "{name} is in neither the table nor the features left out, or in both"
            );
        }
    }

    #[test]
    fn a_set_of_features_crosses_both_ways() {
        let asked = [Feature::ShaderF16, Feature::TextureFormatNv12];
        assert_eq!(features_of(feature_set(&asked)), asked);
    }
}
