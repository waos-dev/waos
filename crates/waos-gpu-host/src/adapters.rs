//! The instance's calls and the adapter: which adapter a program gets, what an adapter says of
//! itself, and the device and queue it opens, which takes memory in the engine's blocks unless the
//! program sets their size ([`MEMORY_BLOCK_BYTES`]).
//!
//! The invariant: a device is opened with limits checked against its adapter's here, whichever
//! way it is opened, and the embedder is told of it before the program is handed it.

use std::sync::Arc;

use wasmtime::component::{Accessor, Resource};
use wgpu_core::id::AdapterId;

use crate::conversions::{feature_set, features_of, label};
use crate::devices::Device;
use crate::limits;
use crate::queues::Queue;
use crate::wasdk::gpu::gpu::{
    self, AdapterInfo, DeviceDescriptor, DownlevelCapabilities, Feature, RequestAdapterOptions,
    TextureFormat, TextureFormatFeatures, WgslLanguageExtensions,
};
use crate::{GpuCtx, GpuObject, HasGpuCtx};

/// An adapter as the program holds it, shared with every device opened on it, which names its
/// GPU.
pub type Adapter = Arc<GpuObject<AdapterId>>;

impl gpu::Host for GpuCtx<'_> {
    fn wgsl_language_features(&mut self) -> wasmtime::Result<WgslLanguageExtensions> {
        use wgpu_core::naga::front::wgsl::ImplementedLanguageExtension as Extension;
        Ok(Extension::all().iter().fold(
            WgslLanguageExtensions::empty(),
            |extensions, extension| {
                extensions
                    | match extension {
                        Extension::ReadOnlyAndReadWriteStorageTextures => {
                            WgslLanguageExtensions::READ_ONLY_AND_READ_WRITE_STORAGE_TEXTURES
                        }
                        Extension::Packed4x8IntegerDotProduct => {
                            WgslLanguageExtensions::PACKED4X8_INTEGER_DOT_PRODUCT
                        }
                        Extension::PointerCompositeAccess => {
                            WgslLanguageExtensions::POINTER_COMPOSITE_ACCESS
                        }
                    }
            },
        ))
    }
}

impl<T: Send + 'static> gpu::HostWithStore<T> for HasGpuCtx {
    async fn request_adapter(
        accessor: &Accessor<T, Self>,
        options: RequestAdapterOptions,
    ) -> wasmtime::Result<Option<Resource<Adapter>>> {
        accessor.with(|mut access| {
            let ctx = access.get();
            let compatible_surface = options
                .compatible_surface
                .map(|surface| ctx.table.get(&surface).map(|surface| surface.target()))
                .transpose()?;
            let core_options = wgpu_types::RequestAdapterOptions {
                power_preference: options.power_preference.into(),
                force_fallback_adapter: options.force_fallback_adapter,
                compatible_surface: None,
                apply_limit_buckets: false,
            };
            let adapter = match ctx.adapters {
                Some(adapters) => adapters.choose(ctx.instance, &core_options),
                None => ctx
                    .instance
                    .request_adapter(&core_options, wgpu_types::Backends::all(), None)
                    .map_err(|error| tracing::warn!("no GPU adapter: {error}"))
                    .ok(),
            };
            let Some(adapter) = adapter else {
                return Ok(None);
            };
            let adapter = Arc::new(GpuObject::new(adapter, ctx.instance));
            // A view's frames come from its target, which says what an adapter draws into it.
            let draws_into_surface = compatible_surface.is_none_or(|target| {
                !target
                    .capabilities(ctx.instance, adapter.id())
                    .formats
                    .is_empty()
            });
            if !draws_into_surface {
                return Ok(None);
            }
            Ok(Some(ctx.table.push(adapter)?))
        })
    }

    async fn enumerate_adapters(
        accessor: &Accessor<T, Self>,
    ) -> wasmtime::Result<Vec<Resource<Adapter>>> {
        accessor.with(|mut access| {
            let ctx = access.get();
            ctx.instance
                .enumerate_adapters(wgpu_types::Backends::all(), false)
                .into_iter()
                .map(|adapter| {
                    Ok(ctx
                        .table
                        .push(Arc::new(GpuObject::new(adapter, ctx.instance)))?)
                })
                .collect()
        })
    }
}

impl gpu::HostAdapter for GpuCtx<'_> {
    fn features(&mut self, adapter: Resource<Adapter>) -> wasmtime::Result<Vec<Feature>> {
        let adapter = self.table.get(&adapter)?.id();
        Ok(features_of(self.instance.adapter_features(adapter)))
    }

    fn limits(
        &mut self,
        adapter: Resource<Adapter>,
    ) -> wasmtime::Result<Resource<wgpu_types::Limits>> {
        let adapter = self.table.get(&adapter)?.id();
        Ok(self.table.push(self.instance.adapter_limits(adapter))?)
    }

    fn info(&mut self, adapter: Resource<Adapter>) -> wasmtime::Result<AdapterInfo> {
        let adapter = self.table.get(&adapter)?.id();
        Ok(self.instance.adapter_get_info(adapter).into())
    }

    fn downlevel_capabilities(
        &mut self,
        adapter: Resource<Adapter>,
    ) -> wasmtime::Result<DownlevelCapabilities> {
        let adapter = self.table.get(&adapter)?.id();
        Ok(self.instance.adapter_downlevel_capabilities(adapter).into())
    }

    fn texture_format_features(
        &mut self,
        adapter: Resource<Adapter>,
        format: TextureFormat,
    ) -> wasmtime::Result<TextureFormatFeatures> {
        let adapter = self.table.get(&adapter)?.id();
        Ok(self
            .instance
            .adapter_get_texture_format_features(adapter, format.into())
            .into())
    }

    fn presentation_timestamp(&mut self, adapter: Resource<Adapter>) -> wasmtime::Result<u64> {
        let adapter = self.table.get(&adapter)?.id();
        Ok(self.instance.adapter_get_presentation_timestamp(adapter).0 as u64)
    }

    fn drop(&mut self, adapter: Resource<Adapter>) -> wasmtime::Result<()> {
        self.table.delete(adapter)?;
        Ok(())
    }
}

impl<T: Send> gpu::HostAdapterWithStore<T> for HasGpuCtx {
    async fn request_device(
        accessor: &Accessor<T, Self>,
        adapter: Resource<Adapter>,
        descriptor: DeviceDescriptor,
    ) -> wasmtime::Result<Result<(Resource<Device>, Resource<Queue>), String>> {
        accessor.with(|mut access| {
            let ctx = access.get();
            let adapter = Arc::clone(ctx.table.get(&adapter)?);
            let adapter_limits = ctx.instance.adapter_limits(adapter.id());
            let required_limits =
                limits::fitted(ctx.table.get(&descriptor.required_limits)?, &adapter_limits);
            if let Some(refusal) = limits::refusal(&required_limits, &adapter_limits) {
                return Ok(Err(refusal));
            }
            let core_descriptor = wgpu_types::DeviceDescriptor {
                label: label(descriptor.label),
                required_features: feature_set(&descriptor.required_features),
                required_limits,
                experimental_features: wgpu_types::ExperimentalFeatures::disabled(),
                memory_hints: memory_hints(descriptor.memory_hints.into()),
                trace: wgpu_types::Trace::Off,
            };
            let opened = match ctx.device_opener {
                Some(opener) => opener.open(ctx.instance, adapter.id(), &core_descriptor),
                None => {
                    ctx.instance
                        .adapter_request_device(adapter.id(), &core_descriptor, None, None)
                }
            };
            let (device, queue) = match opened {
                Ok(opened) => opened,
                Err(refusal) => return Ok(Err(refusal.to_string())),
            };
            let device = Device::opened(ctx.instance, adapter, device, queue);
            if let Some(devices) = ctx.devices {
                devices.opened(&device);
            }
            let queue = Queue::of(&device);
            Ok(Ok((ctx.table.push(device)?, ctx.table.push(queue)?)))
        })
    }
}

/// The size of the blocks a program's device takes memory from the system in, on Vulkan and
/// DX12: what Impeller asks of its allocator, and Skia's default (`allocator_vk.cc`,
/// `preferredLargeHeapBlockSize`). WaOS runs many programs at once, so what each holds in blocks
/// it has not filled matters more than how often it asks the system for one: wgpu's own hints
/// take 128 to 256 MB at a time and give a block back as it empties, and a 15 MB texture that
/// came and went with a blur cost a Pixel 2 XL a 256 MB allocation, 134 to 225 ms, each time.
const MEMORY_BLOCK_BYTES: u64 = 4 * 1024 * 1024;

/// How a device the program opens takes memory: wgpu's two hints, `Performance`, its default,
/// and `MemoryUsage`, in the engine's blocks of [`MEMORY_BLOCK_BYTES`], with a block of its own
/// for anything larger; the block sizes a program sets itself, as it sets them.
fn memory_hints(asked: wgpu_types::MemoryHints) -> wgpu_types::MemoryHints {
    match asked {
        wgpu_types::MemoryHints::Performance | wgpu_types::MemoryHints::MemoryUsage => {
            wgpu_types::MemoryHints::Manual {
                suballocated_device_memory_block_size: MEMORY_BLOCK_BYTES..MEMORY_BLOCK_BYTES,
            }
        }
        manual @ wgpu_types::MemoryHints::Manual { .. } => manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_sizes(hints: wgpu_types::MemoryHints) -> std::ops::Range<u64> {
        match hints {
            wgpu_types::MemoryHints::Manual {
                suballocated_device_memory_block_size,
            } => suballocated_device_memory_block_size,
            other => panic!("{other:?} is no block size"),
        }
    }

    #[test]
    fn wgpus_hints_take_memory_in_the_engines_blocks() {
        for asked in [
            wgpu_types::MemoryHints::Performance,
            wgpu_types::MemoryHints::MemoryUsage,
        ] {
            assert_eq!(
                block_sizes(memory_hints(asked)),
                MEMORY_BLOCK_BYTES..MEMORY_BLOCK_BYTES
            );
        }
    }

    #[test]
    fn block_sizes_the_program_sets_are_kept() {
        let asked = wgpu_types::MemoryHints::Manual {
            suballocated_device_memory_block_size: (1 << 20)..(64 << 20),
        };
        assert_eq!(block_sizes(memory_hints(asked)), (1 << 20)..(64 << 20));
    }
}
