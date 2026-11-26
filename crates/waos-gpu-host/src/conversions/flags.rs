//! The flag sets, flag by flag, each named as the other crate names it: to wgpu-types for what a
//! program asks, back for what an adapter or a texture answers. A flag of wgpu's with no case here
//! is one the package leaves out (ray tracing's usages, the experimental stages).

use wgpu_types as wgt;

use super::rows;
use crate::wasdk::gpu::types;

macro_rules! to_wgpu {
    ($wit:ident => $wgt:ident { $($rows:tt)* }) => {
        rows!(to_wgpu_impl! ($wit $wgt) [] $($rows)*);
    };
}

macro_rules! to_wgpu_impl {
    (($wit:ident $wgt:ident) [$(($a:ident $b:ident))*]) => {
        impl From<types::$wit> for wgt::$wgt {
            fn from(value: types::$wit) -> Self {
                let mut flags = wgt::$wgt::empty();
                $(if value.contains(types::$wit::$a) {
                    flags |= wgt::$wgt::$b;
                })*
                flags
            }
        }
    };
}

macro_rules! from_wgpu {
    ($wit:ident <= $wgt:ident { $($rows:tt)* }) => {
        rows!(from_wgpu_impl! ($wit $wgt) [] $($rows)*);
    };
}

macro_rules! from_wgpu_impl {
    (($wit:ident $wgt:ident) [$(($a:ident $b:ident))*]) => {
        impl From<wgt::$wgt> for types::$wit {
            fn from(value: wgt::$wgt) -> Self {
                let mut flags = types::$wit::empty();
                $(if value.contains(wgt::$wgt::$b) {
                    flags |= types::$wit::$a;
                })*
                flags
            }
        }
    };
}

to_wgpu!(TextureUsages => TextureUsages {
    COPY_SRC, COPY_DST, TEXTURE_BINDING, STORAGE_BINDING, RENDER_ATTACHMENT, TRANSIENT_ATTACHMENT,
    STORAGE_ATOMIC,
});
from_wgpu!(
    TextureUsages
        <= TextureUsages {
            COPY_SRC,
            COPY_DST,
            TEXTURE_BINDING,
            STORAGE_BINDING,
            RENDER_ATTACHMENT,
            TRANSIENT_ATTACHMENT,
            STORAGE_ATOMIC,
        }
);
to_wgpu!(BufferUsages => BufferUsages {
    MAP_READ, MAP_WRITE, COPY_SRC, COPY_DST, INDEX, VERTEX, UNIFORM, STORAGE, INDIRECT,
    QUERY_RESOLVE,
});
to_wgpu!(ShaderStages => ShaderStages { VERTEX, FRAGMENT, COMPUTE });
to_wgpu!(ColorWrites => ColorWrites { RED, GREEN, BLUE, ALPHA });
to_wgpu!(PipelineStatisticsTypes => PipelineStatisticsTypes {
    VERTEX_SHADER_INVOCATIONS, CLIPPER_INVOCATIONS, CLIPPER_PRIMITIVES_OUT,
    FRAGMENT_SHADER_INVOCATIONS, COMPUTE_SHADER_INVOCATIONS,
});
from_wgpu!(
    TextureFormatFeatureFlags
        <= TextureFormatFeatureFlags {
            FILTERABLE,
            MULTISAMPLE_X2,
            MULTISAMPLE_X4,
            MULTISAMPLE_X8,
            MULTISAMPLE_X16,
            MULTISAMPLE_RESOLVE,
            STORAGE_READ_ONLY,
            STORAGE_WRITE_ONLY,
            STORAGE_READ_WRITE,
            STORAGE_ATOMIC,
            BLENDABLE,
        }
);
from_wgpu!(DownlevelFlags <= DownlevelFlags {
    COMPUTE_SHADERS, FRAGMENT_WRITABLE_STORAGE, INDIRECT_EXECUTION, BASE_VERTEX,
    READ_ONLY_DEPTH_STENCIL, NON_POWER_OF_TWO_MIPMAPPED_TEXTURES, CUBE_ARRAY_TEXTURES,
    COMPARISON_SAMPLERS, INDEPENDENT_BLEND, VERTEX_STORAGE, ANISOTROPIC_FILTERING, FRAGMENT_STORAGE,
    MULTISAMPLED_SHADING, DEPTH_TEXTURE_AND_BUFFER_COPIES, WEBGPU_TEXTURE_FORMAT_SUPPORT,
    BUFFER_BINDINGS_NOT16_BYTE_ALIGNED = BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED,
    UNRESTRICTED_INDEX_BUFFER, FULL_DRAW_INDEX_UINT32,
});
