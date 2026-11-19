//! How a program's device is opened, and the engine's own, where Vulkan imports the system's
//! hardware buffers: through wgpu-hal's `open_with_callback`, with the system's import extensions
//! (`os::IMPORT_EXTENSIONS`: on Android `VK_ANDROID_external_memory_android_hardware_buffer`, on
//! Linux the dmabuf's) and `VK_EXT_queue_family_foreign` added to the extensions wgpu-hal enables,
//! which it never adds itself. Every other part of the device is what wgpu-hal opens, so
//! wgpu-core's device over it is any wgpu device.
//!
//! The invariant: a device imports hardware buffers only when it was opened with every one of
//! them (`imports`), since an import into a device without them is undefined; a device on an
//! adapter that cannot have them is wgpu-core's own, and its canvas then fails to configure,
//! saying why.

use waos_webgpu_host::DeviceOpener;
use wgpu_core::id::{AdapterId, DeviceId, QueueId};
use wgpu_core::instance::RequestDeviceError;
use wgpu_hal::api::Vulkan;

use crate::gpu::{wgpu_core, wgpu_types};

use crate::os::IMPORT_EXTENSIONS;

/// How a program's device is opened: on Vulkan, with the extensions that import a hardware
/// buffer's memory.
pub(crate) struct HardwareBufferImports;

impl DeviceOpener for HardwareBufferImports {
    /// The device `descriptor` asks for on `adapter`, with the import's extensions where the
    /// adapter is Vulkan's and has them, and wgpu-core's own device otherwise. A feature the adapter
    /// lacks is refused, as wgpu-core refuses it.
    fn open(
        &self,
        instance: &wgpu_core::global::Global,
        adapter: AdapterId,
        descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
    ) -> Result<(DeviceId, QueueId), RequestDeviceError> {
        let missing = descriptor.required_features - instance.adapter_features(adapter);
        if !missing.is_empty() {
            return Err(RequestDeviceError::UnsupportedFeature(missing));
        }
        if !can_import(instance, adapter) {
            tracing::warn!(
                "the GPU adapter cannot import hardware buffers: a program's canvas on it will not configure"
            );
            return instance.adapter_request_device(adapter, descriptor, None, None);
        }
        let opened = open_with_imports(instance, adapter, descriptor)?;
        // SAFETY: the device was opened on this adapter's physical device, with the descriptor's
        // features and limits, which the adapter has.
        unsafe { instance.create_device_from_hal(adapter, opened, descriptor, None, None) }
    }
}

/// The hal device `descriptor` asks for on `adapter`, Vulkan's, with the import's extensions.
fn open_with_imports(
    instance: &wgpu_core::global::Global,
    adapter: AdapterId,
    descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
) -> Result<wgpu_hal::DynOpenDevice, RequestDeviceError> {
    // SAFETY: the hal adapter is only read, and let go before wgpu-core takes the device.
    let hal = unsafe { instance.adapter_as_hal::<Vulkan>(adapter) }.ok_or(
        RequestDeviceError::Device(wgpu_core::device::DeviceError::Lost),
    )?;
    // SAFETY: the features and limits are the descriptor's, which the adapter has (checked by the
    // caller and by the WebGPU host); the callback only adds two extensions the adapter offers
    // (`can_import`).
    let opened = unsafe {
        hal.open_with_callback(
            descriptor.required_features,
            &descriptor.required_limits,
            &descriptor.memory_hints,
            Some(Box::new(|arguments| {
                for extension in IMPORT_EXTENSIONS.iter().copied() {
                    if !arguments.extensions.contains(&extension) {
                        arguments.extensions.push(extension);
                    }
                }
            })),
        )
    }
    .map_err(|error| RequestDeviceError::Device(wgpu_core::device::DeviceError::from_hal(error)))?;
    Ok(opened.into())
}

/// Whether `adapter` is Vulkan's, at 1.1 or later, whose dedicated allocations an import takes,
/// and offers both extensions.
fn can_import(instance: &wgpu_core::global::Global, adapter: AdapterId) -> bool {
    // SAFETY: the adapter is only read, and let go before this returns.
    let Some(hal) = (unsafe { instance.adapter_as_hal::<Vulkan>(adapter) }) else {
        return false;
    };
    let capabilities = hal.physical_device_capabilities();
    capabilities.properties().api_version >= ash::vk::API_VERSION_1_1
        && IMPORT_EXTENSIONS
            .iter()
            .all(|extension| capabilities.supports_extension(extension))
}

/// Whether the default adapter on `instance` is a Vulkan adapter capable of importing hardware
/// buffers.
#[cfg(target_os = "linux")]
pub(crate) fn adapter_can_import(instance: &crate::gpu::GpuInstance) -> bool {
    let Ok(adapter) = instance.request_adapter(
        &wgpu_types::RequestAdapterOptions::default(),
        wgpu_types::Backends::all(),
        None,
    ) else {
        return false;
    };
    let adapter = waos_webgpu_host::GpuObject::new(adapter, instance);
    can_import(instance, adapter.id())
}

/// Whether `device`, a hal device, was opened with the extensions that import a hardware
/// buffer's memory.
pub(crate) fn imports(device: &wgpu_hal::vulkan::Device) -> bool {
    let enabled = device.enabled_device_extensions();
    IMPORT_EXTENSIONS
        .iter()
        .all(|extension| enabled.contains(extension))
}
