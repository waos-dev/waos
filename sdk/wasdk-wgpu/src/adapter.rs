//! The adapter: what it has, and the device and queue it opens.

use std::pin::Pin;
use std::sync::Arc;

use wasdk::gpu::{Adapter, DeviceDescriptor};
use wgpu::custom::{
    AdapterInterface, DispatchDevice, DispatchQueue, DispatchSurface, RequestDeviceFuture,
};
use wgpu::{
    AdapterInfo, DownlevelCapabilities, Features, Limits, PresentationTimestamp,
    RequestDeviceError, TextureFormat, TextureFormatFeatures,
};

use crate::convert;
use crate::device::WasdkDevice;
use crate::queue::WasdkQueue;
use crate::runtime::OwedCallbacks;
use crate::surface::WasdkSurface;

/// The adapter behind a [`wgpu::Adapter`] from this backend.
#[derive(Debug)]
pub struct WasdkAdapter {
    adapter: Arc<Adapter>,
}

downcast!(WasdkAdapter: dispatch wgpu::custom::DispatchAdapter);

impl WasdkAdapter {
    pub(crate) fn new(adapter: Adapter) -> Self {
        WasdkAdapter {
            adapter: Arc::new(adapter),
        }
    }

    /// The engine's adapter behind the wgpu one, which a surface's capabilities are asked on.
    pub(crate) fn wit_adapter(&self) -> &Adapter {
        &self.adapter
    }
}

impl AdapterInterface for WasdkAdapter {
    /// The device and its queue. Required features the WIT leaves out fail it without asking the
    /// engine, through the future; the trace, a file of the engine's, and the experimental
    /// features, which the WIT leaves out, are not passed on.
    fn request_device(
        &self,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Pin<Box<dyn RequestDeviceFuture>> {
        let required_features =
            convert::features_to_wit(desc.required_features).map_err(|left_out| {
                RequestDeviceError::from_message(format!(
                    "wasdk:gpu leaves out the required {left_out:?}"
                ))
            });
        let required_limits = convert::limits_to_wit(&desc.required_limits);
        let label = convert::label(desc.label);
        let memory_hints = convert::memory_hints_to_wit(&desc.memory_hints);
        let adapter = Arc::clone(&self.adapter);
        Box::pin(async move {
            let descriptor = DeviceDescriptor {
                label,
                required_features: required_features?,
                required_limits: &required_limits,
                memory_hints,
            };
            let (device, queue) = adapter
                .request_device(descriptor)
                .await
                .map_err(RequestDeviceError::from_message)?;
            let owed = OwedCallbacks::default();
            let device = WasdkDevice::new(device, owed.clone());
            // The device's listener goes to the executor awaiting this future
            // (`uncaptured_errors`).
            wasdk::yield_async().await;
            Ok((
                DispatchDevice::custom(device),
                DispatchQueue::custom(WasdkQueue::new(queue, owed)),
            ))
        })
    }

    /// A surface is supported where its capabilities on this adapter have a format.
    fn is_surface_supported(&self, surface: &DispatchSurface) -> bool {
        let surface = WasdkSurface::of_dispatch(surface).wit_surface();
        !surface.capabilities(&self.adapter).formats.is_empty()
    }

    fn features(&self) -> Features {
        convert::features_from_wit(self.adapter.features())
    }

    fn limits(&self) -> Limits {
        convert::limits_from_wit(&self.adapter.limits())
    }

    fn downlevel_capabilities(&self) -> DownlevelCapabilities {
        convert::downlevel_capabilities_from_wit(self.adapter.downlevel_capabilities())
    }

    fn get_info(&self) -> AdapterInfo {
        convert::adapter_info_from_wit(self.adapter.info())
    }

    fn get_texture_format_features(&self, format: TextureFormat) -> TextureFormatFeatures {
        let format = convert::texture_format_to_wit(format);
        convert::texture_format_features_from_wit(self.adapter.texture_format_features(format))
    }

    fn get_presentation_timestamp(&self) -> PresentationTimestamp {
        convert::presentation_timestamp_from_wit(self.adapter.presentation_timestamp())
    }

    /// Cooperative matrices are experimental, and left out of the WIT.
    fn cooperative_matrix_properties(&self) -> Vec<wgpu::wgt::CooperativeMatrixProperties> {
        Vec::new()
    }
}
