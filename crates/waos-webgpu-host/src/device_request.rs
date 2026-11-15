//! What a program asks a device for, and how `request-device` refuses it, as WebGPU's
//! `requestDevice` does: a feature the adapter does not have is a `TypeError`; a limit that is not
//! a limit, better than the adapter's, or an alignment that is not a power of two below 2^32 is an
//! `OperationError`. https://www.w3.org/TR/webgpu/#dom-gpuadapter-requestdevice
//!
//! One refusal is not WebGPU's: when the adapter cannot make a device at all, WebGPU resolves with
//! a device that is already lost, and wgpu-core makes no device to lose, so the host answers an
//! `OperationError` with wgpu-core's reason.
//!
//! And one adapter is not WebGPU's: one that falls below WebGPU's default limits, as the iOS
//! simulator's Metal does with 15 inter-stage shader variables against 16, which WebGPU would not
//! offer. A device there starts from the adapter's own value for each limit it falls short on, so
//! a program that asks for no more than WebGPU's defaults still draws, wgpu's programs among them,
//! which ask for every default by value; and one whose shader needs more fails at its pipeline
//! with wgpu's validation error; as Waft asks valo for a device on a compliant adapter, or
//! else on any adapter valo draws with (`valo::request_device`).
//!
//! What was asked is checked here; the device itself is made by wgpu-core's
//! `adapter_request_device`, or by the embedder's [`DeviceOpener`] when it gives one.

use wasmtime::component::ResourceTable;

use crate::wasi::webgpu::webgpu;

/// How an embedder opens a program's device in place of wgpu-core's `adapter_request_device`.
/// An embedder whose frames the system compositor reads needs the device made with backend
/// extensions wgpu-core does not enable, which only a device opened through the backend itself
/// and handed to `Global::create_device_from_hal` can have.
///
/// `descriptor` has been checked against the adapter's limits, as WebGPU checks them; its
/// features have not, and `create_device_from_hal` does not check them either, so an opener
/// refuses a feature the adapter lacks with `RequestDeviceError::UnsupportedFeature`. Whatever
/// the opener refuses reaches the program as `request-device`'s refusal.
pub trait DeviceOpener: Send + Sync {
    fn open(
        &self,
        instance: &wgpu_core::global::Global,
        adapter: wgpu_core::id::AdapterId,
        descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
    ) -> Result<
        (wgpu_core::id::DeviceId, wgpu_core::id::QueueId),
        wgpu_core::instance::RequestDeviceError,
    >;
}

/// The size of the blocks a program's device takes memory from the system in, on Vulkan and
/// DX12: what Impeller asks of its allocator, and Skia's default (`allocator_vk.cc`,
/// `preferredLargeHeapBlockSize`).
const MEMORY_BLOCK_BYTES: u64 = 4 * 1024 * 1024;

/// How a program's device takes memory from the system, which WebGPU leaves to the host: in
/// blocks of one small size, and a block of its own for anything larger, so a texture that
/// comes costs an allocation of its size. wgpu's default takes 128 to 256 MB at a time and
/// gives a block back as it empties: a 15 MB texture that came and went with a blur cost a
/// Pixel 2 XL a 256 MB allocation, 134 to 225 ms, each time.
const MEMORY_HINTS: wgpu_types::MemoryHints = wgpu_types::MemoryHints::Manual {
    suballocated_device_memory_block_size: MEMORY_BLOCK_BYTES..MEMORY_BLOCK_BYTES,
};

/// The descriptor wgpu-core makes the device from, or the refusal of what `descriptor` asks.
pub(crate) fn device_descriptor(
    descriptor: Option<webgpu::GpuDeviceDescriptor>,
    table: &ResourceTable,
    adapter_limits: &wgpu_types::Limits,
) -> Result<wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>, webgpu::RequestDeviceError> {
    let Some(descriptor) = descriptor else {
        return Ok(wgpu_types::DeviceDescriptor {
            required_limits: baseline_limits(adapter_limits),
            memory_hints: MEMORY_HINTS,
            ..Default::default()
        });
    };
    let required_limits = match &descriptor.required_limits {
        Some(record) => {
            let record = table
                .get(record)
                .map_err(|error| operation_error(error.to_string()))?;
            required_limits(record, adapter_limits)?
        }
        None => baseline_limits(adapter_limits),
    };
    Ok(wgpu_types::DeviceDescriptor {
        label: descriptor.label.map(|label| label.into()),
        required_features: required_features(descriptor.required_features.unwrap_or_default())?,
        required_limits,
        // TODO: use descriptor.default_queue?
        memory_hints: MEMORY_HINTS,
        // trace is not present in WebGPU
        trace: wgpu_types::Trace::default(),
        // Don't enable any experimental features
        experimental_features: wgpu_types::ExperimentalFeatures::disabled(),
    })
}

/// How wgpu-core's refusal to make the device reaches the program.
pub(crate) fn refusal(
    error: wgpu_core::instance::RequestDeviceError,
) -> webgpu::RequestDeviceError {
    let message = error.to_string();
    match error {
        wgpu_core::instance::RequestDeviceError::UnsupportedFeature(_) => {
            webgpu::RequestDeviceError {
                kind: webgpu::RequestDeviceErrorKind::TypeError,
                message,
            }
        }
        // Every other refusal is an `OperationError`: a limit beyond the adapter's is WebGPU's,
        // and a device the adapter could not make is the host's stand-in for a device already
        // lost, as the module says.
        _ => operation_error(message),
    }
}

fn operation_error(message: String) -> webgpu::RequestDeviceError {
    webgpu::RequestDeviceError {
        kind: webgpu::RequestDeviceErrorKind::OperationError,
        message,
    }
}

/// The features named, or the `TypeError` for one the adapter cannot have, as a feature wgpu has
/// no WebGPU feature for is never among the adapter's.
fn required_features(
    names: Vec<webgpu::GpuFeatureName>,
) -> Result<wgpu_types::Features, webgpu::RequestDeviceError> {
    let features_webgpu = names
        .into_iter()
        .map(|name| {
            webgpu_feature(name).ok_or_else(|| webgpu::RequestDeviceError {
                kind: webgpu::RequestDeviceErrorKind::TypeError,
                message: format!("the adapter does not have the feature {name:?}"),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(wgpu_types::Features {
        features_webgpu,
        // Don't enable any native features
        features_wgpu: wgpu_types::FeaturesWGPU::empty(),
    })
}

/// wgpu's name for a WebGPU feature, where it has the feature.
fn webgpu_feature(name: webgpu::GpuFeatureName) -> Option<wgpu_types::FeaturesWebGPU> {
    use webgpu::GpuFeatureName as Name;
    use wgpu_types::FeaturesWebGPU as Feature;
    Some(match name {
        Name::DepthClipControl => Feature::DEPTH_CLIP_CONTROL,
        Name::Depth32floatStencil8 => Feature::DEPTH32FLOAT_STENCIL8,
        Name::TextureCompressionBc => Feature::TEXTURE_COMPRESSION_BC,
        Name::TextureCompressionBcSliced3d => Feature::TEXTURE_COMPRESSION_BC_SLICED_3D,
        Name::TextureCompressionEtc2 => Feature::TEXTURE_COMPRESSION_ETC2,
        Name::TextureCompressionAstc => Feature::TEXTURE_COMPRESSION_ASTC,
        Name::TextureCompressionAstcSliced3d => Feature::TEXTURE_COMPRESSION_ASTC_SLICED_3D,
        Name::TimestampQuery => Feature::TIMESTAMP_QUERY,
        Name::IndirectFirstInstance => Feature::INDIRECT_FIRST_INSTANCE,
        Name::ShaderF16 => Feature::SHADER_F16,
        Name::Rg11b10ufloatRenderable => Feature::RG11B10UFLOAT_RENDERABLE,
        Name::Bgra8unormStorage => Feature::BGRA8UNORM_STORAGE,
        Name::Float32Filterable => Feature::FLOAT32_FILTERABLE,
        Name::Float32Blendable => Feature::FLOAT32_BLENDABLE,
        Name::ClipDistances => Feature::CLIP_DISTANCES,
        Name::DualSourceBlending => Feature::DUAL_SOURCE_BLENDING,
        Name::PrimitiveIndex => Feature::PRIMITIVE_INDEX,
        // Features the host's adapters never report, since wgpu has no WebGPU feature for them.
        Name::CoreFeaturesAndLimits
        | Name::Subgroups
        | Name::TextureFormatsTier1
        | Name::TextureFormatsTier2
        | Name::TextureComponentSwizzle => return None,
    })
}

/// The device's limits: the baseline ([`baseline_limits`]), with each better value `record` asks
/// for, or the `OperationError` for a key that is not a limit or a value the adapter does not
/// allow.
fn required_limits(
    record: &webgpu::RecordOptionGpuSize64,
    adapter_limits: &wgpu_types::Limits,
) -> Result<wgpu_types::Limits, webgpu::RequestDeviceError> {
    let mut limits = baseline_limits(adapter_limits);
    for (key, value) in record {
        let slot =
            limit_slot(key).ok_or_else(|| operation_error(format!("{key} is not a limit")))?;
        if let Some(value) = value {
            slot.ask(&mut limits, adapter_limits, *value)
                .map_err(|refused| operation_error(format!("{key}: {refused}")))?;
        }
    }
    Ok(limits)
}

/// What a device has before a program asks for more: WebGPU's defaults, and where the adapter falls
/// below them, the adapter's own value for each limit it falls short on, which one line names.
fn baseline_limits(adapter_limits: &wgpu_types::Limits) -> wgpu_types::Limits {
    let defaults = wgpu_types::Limits::defaults();
    let mut short = Vec::new();
    defaults.check_limits_with_fail_fn(adapter_limits, false, |name, default, adapter| {
        short.push(format!("{name} {adapter} (WebGPU's default {default})"));
    });
    if !short.is_empty() {
        tracing::warn!(
            "the adapter falls below WebGPU's default limits; a device has its own: {}",
            short.join(", ")
        );
    }
    defaults.or_worse_values_from(adapter_limits)
}

/// Where a WebGPU limit is in wgpu's limits, and which way a value of it is better.
enum LimitSlot {
    /// A limit of which a larger value is better.
    Maximum32(fn(&mut wgpu_types::Limits) -> &mut u32),
    Maximum64(fn(&mut wgpu_types::Limits) -> &mut u64),
    /// A limit of which a smaller value is better, and every value a power of two.
    Alignment(fn(&mut wgpu_types::Limits) -> &mut u32),
    /// A maximum wgpu does not have separately, which the host's adapters answer with the value
    /// of the wgpu limit given.
    AnsweredBy(fn(&wgpu_types::Limits) -> u32),
}

impl LimitSlot {
    /// Sets `value` in `limits` where it is better than what is there, or refuses it when it is
    /// better than the adapter's. A value no better than WebGPU's default is what the baseline
    /// gives already ([`baseline_limits`]), and is never refused.
    fn ask(
        &self,
        limits: &mut wgpu_types::Limits,
        adapter_limits: &wgpu_types::Limits,
        value: u64,
    ) -> Result<(), String> {
        let beyond_adapter = || format!("{value} is better than the adapter's limit");
        let mut adapter = adapter_limits.clone();
        let mut defaults = wgpu_types::Limits::defaults();
        match self {
            LimitSlot::Maximum32(slot) => {
                let value = u32::try_from(value).map_err(|_| beyond_adapter())?;
                if value <= *slot(&mut defaults) {
                    return Ok(());
                }
                if value > *slot(&mut adapter) {
                    return Err(beyond_adapter());
                }
                let current = slot(limits);
                *current = (*current).max(value);
            }
            LimitSlot::Maximum64(slot) => {
                if value <= *slot(&mut defaults) {
                    return Ok(());
                }
                if value > *slot(&mut adapter) {
                    return Err(beyond_adapter());
                }
                let current = slot(limits);
                *current = (*current).max(value);
            }
            LimitSlot::Alignment(slot) => {
                let value = u32::try_from(value)
                    .ok()
                    .filter(|value| value.is_power_of_two())
                    .ok_or_else(|| format!("{value} is not a power of two below 2^32"))?;
                if value >= *slot(&mut defaults) {
                    return Ok(());
                }
                if value < *slot(&mut adapter) {
                    return Err(beyond_adapter());
                }
                let current = slot(limits);
                *current = (*current).min(value);
            }
            LimitSlot::AnsweredBy(answer) => {
                if value > u64::from(answer(&defaults)) && value > u64::from(answer(adapter_limits))
                {
                    return Err(beyond_adapter());
                }
            }
        }
        Ok(())
    }
}

/// Where the limit `key` names is, its case and dashes aside; none for a key that is not a limit.
fn limit_slot(key: &str) -> Option<LimitSlot> {
    use LimitSlot::{Alignment, AnsweredBy, Maximum32, Maximum64};
    let key = key.replace('-', "").to_lowercase();
    Some(match key.as_str() {
        "maxtexturedimension1d" => Maximum32(|l| &mut l.max_texture_dimension_1d),
        "maxtexturedimension2d" => Maximum32(|l| &mut l.max_texture_dimension_2d),
        "maxtexturedimension3d" => Maximum32(|l| &mut l.max_texture_dimension_3d),
        "maxtexturearraylayers" => Maximum32(|l| &mut l.max_texture_array_layers),
        "maxbindgroups" => Maximum32(|l| &mut l.max_bind_groups),
        "maxbindgroupsplusvertexbuffers" => {
            Maximum32(|l| &mut l.max_bind_groups_plus_vertex_buffers)
        }
        "maxbindingsperbindgroup" => Maximum32(|l| &mut l.max_bindings_per_bind_group),
        "maxdynamicuniformbuffersperpipelinelayout" => {
            Maximum32(|l| &mut l.max_dynamic_uniform_buffers_per_pipeline_layout)
        }
        "maxdynamicstoragebuffersperpipelinelayout" => {
            Maximum32(|l| &mut l.max_dynamic_storage_buffers_per_pipeline_layout)
        }
        "maxsampledtexturespershaderstage" => {
            Maximum32(|l| &mut l.max_sampled_textures_per_shader_stage)
        }
        "maxsamplerspershaderstage" => Maximum32(|l| &mut l.max_samplers_per_shader_stage),
        "maxstoragebufferspershaderstage" => {
            Maximum32(|l| &mut l.max_storage_buffers_per_shader_stage)
        }
        "maxstoragebuffersinvertexstage" | "maxstoragebuffersinfragmentstage" => {
            AnsweredBy(|l| l.max_storage_buffers_per_shader_stage)
        }
        "maxstoragetexturespershaderstage" => {
            Maximum32(|l| &mut l.max_storage_textures_per_shader_stage)
        }
        "maxstoragetexturesinvertexstage" | "maxstoragetexturesinfragmentstage" => {
            AnsweredBy(|l| l.max_storage_textures_per_shader_stage)
        }
        "maxuniformbufferspershaderstage" => {
            Maximum32(|l| &mut l.max_uniform_buffers_per_shader_stage)
        }
        "maxuniformbufferbindingsize" => Maximum64(|l| &mut l.max_uniform_buffer_binding_size),
        "maxstoragebufferbindingsize" => Maximum64(|l| &mut l.max_storage_buffer_binding_size),
        "minuniformbufferoffsetalignment" => {
            Alignment(|l| &mut l.min_uniform_buffer_offset_alignment)
        }
        "minstoragebufferoffsetalignment" => {
            Alignment(|l| &mut l.min_storage_buffer_offset_alignment)
        }
        "maxvertexbuffers" => Maximum32(|l| &mut l.max_vertex_buffers),
        "maxbuffersize" => Maximum64(|l| &mut l.max_buffer_size),
        "maxvertexattributes" => Maximum32(|l| &mut l.max_vertex_attributes),
        "maxvertexbufferarraystride" => Maximum32(|l| &mut l.max_vertex_buffer_array_stride),
        "maxinterstageshadervariables" => Maximum32(|l| &mut l.max_inter_stage_shader_variables),
        "maxcolorattachments" => Maximum32(|l| &mut l.max_color_attachments),
        "maxcolorattachmentbytespersample" => {
            Maximum32(|l| &mut l.max_color_attachment_bytes_per_sample)
        }
        "maxcomputeworkgroupstoragesize" => {
            Maximum32(|l| &mut l.max_compute_workgroup_storage_size)
        }
        "maxcomputeinvocationsperworkgroup" => {
            Maximum32(|l| &mut l.max_compute_invocations_per_workgroup)
        }
        "maxcomputeworkgroupsizex" => Maximum32(|l| &mut l.max_compute_workgroup_size_x),
        "maxcomputeworkgroupsizey" => Maximum32(|l| &mut l.max_compute_workgroup_size_y),
        "maxcomputeworkgroupsizez" => Maximum32(|l| &mut l.max_compute_workgroup_size_z),
        "maxcomputeworkgroupsperdimension" => {
            Maximum32(|l| &mut l.max_compute_workgroups_per_dimension)
        }
        "maximmediatesize" => Maximum32(|l| &mut l.max_immediate_size),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The iOS simulator's Metal adapter: WebGPU's defaults but for 15 inter-stage shader
    /// variables.
    fn simulator_limits() -> wgpu_types::Limits {
        wgpu_types::Limits {
            max_inter_stage_shader_variables: 15,
            ..wgpu_types::Limits::defaults()
        }
    }

    #[test]
    fn a_device_on_an_adapter_below_webgpus_defaults_has_the_adapters_value_for_what_falls_short() {
        let table = ResourceTable::new();
        let undescribed =
            device_descriptor(None, &table, &simulator_limits()).expect("nothing asked is refused");
        assert_eq!(undescribed.required_limits, simulator_limits());
        let generous = wgpu_types::Limits {
            max_inter_stage_shader_variables: 31,
            ..wgpu_types::Limits::defaults()
        };
        let compliant =
            device_descriptor(None, &table, &generous).expect("nothing asked is refused");
        assert_eq!(
            compliant.required_limits,
            wgpu_types::Limits::defaults(),
            "WebGPU's defaults where the adapter meets them, never more"
        );
    }

    /// As a wgpu program asks, every limit at WebGPU's default by value.
    #[test]
    fn webgpus_default_asked_of_an_adapter_below_it_gives_the_adapters_value() {
        let mut asked = webgpu::RecordOptionGpuSize64::new();
        asked.insert("maxInterStageShaderVariables".to_owned(), Some(16));
        asked.insert("maxTextureDimension2D".to_owned(), Some(8192));
        let limits = required_limits(&asked, &simulator_limits()).expect("the defaults are given");
        assert_eq!(limits, simulator_limits());
    }

    #[test]
    fn a_limit_asked_beyond_webgpus_default_and_the_adapters_is_refused() {
        let mut asked = webgpu::RecordOptionGpuSize64::new();
        asked.insert("maxInterStageShaderVariables".to_owned(), Some(17));
        assert!(required_limits(&asked, &simulator_limits()).is_err());
    }

    fn takes_memory_in_small_blocks(
        descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
    ) -> bool {
        matches!(
            &descriptor.memory_hints,
            wgpu_types::MemoryHints::Manual { suballocated_device_memory_block_size: blocks }
                if blocks.start == MEMORY_BLOCK_BYTES && blocks.end == MEMORY_BLOCK_BYTES
        )
    }

    /// wgpu's own default, which a descriptor filled in from `Default` would carry, takes
    /// memory 128 to 256 MB at a time.
    #[test]
    fn a_device_takes_memory_in_small_blocks_whether_or_not_the_program_describes_it() {
        let (table, limits) = (ResourceTable::new(), wgpu_types::Limits::defaults());
        let undescribed =
            device_descriptor(None, &table, &limits).expect("nothing asked is refused");
        assert!(takes_memory_in_small_blocks(&undescribed));
        let described = webgpu::GpuDeviceDescriptor {
            required_features: None,
            required_limits: None,
            default_queue: None,
            label: None,
        };
        let described =
            device_descriptor(Some(described), &table, &limits).expect("nothing asked is refused");
        assert!(takes_memory_in_small_blocks(&described));
    }
}
