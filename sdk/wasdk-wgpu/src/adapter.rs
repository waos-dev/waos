//! The adapter: capabilities, and the device it opens.

use std::pin::Pin;
use std::sync::Arc;

use wasdk::webgpu::{GpuAdapter, GpuDeviceDescriptor};
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
use crate::surface::WasdkSurface;

#[derive(Debug)]
pub struct WasdkAdapter {
    adapter: Arc<GpuAdapter>,
}

impl WasdkAdapter {
    pub(crate) fn new(adapter: GpuAdapter) -> Self {
        WasdkAdapter {
            adapter: Arc::new(adapter),
        }
    }
}

impl AdapterInterface for WasdkAdapter {
    fn request_device(
        &self,
        desc: &wgpu::DeviceDescriptor<'_>,
    ) -> Pin<Box<dyn RequestDeviceFuture>> {
        // The descriptor is built before the future so a feature wasi:webgpu cannot name is
        // reported without a round trip; the error still travels through the future.
        let descriptor = convert::features_to_wasi(desc.required_features)
            .map(|required_features| GpuDeviceDescriptor {
                required_features: Some(required_features),
                required_limits: Some(convert::limits_to_wasi(&desc.required_limits)),
                default_queue: None,
                label: convert::label(desc.label),
            })
            .map_err(|unsupported| {
                RequestDeviceError::from_message(format!(
                    "wasi:webgpu has no name for the required {unsupported:?}"
                ))
            });
        let adapter = Arc::clone(&self.adapter);
        Box::pin(async move {
            let device = adapter
                .request_device(Some(descriptor?))
                .await
                .map_err(|error| {
                    RequestDeviceError::from_message(format!("{:?}: {}", error.kind, error.message))
                })?;
            let queue = device.queue();
            Ok((
                DispatchDevice::custom(WasdkDevice::new(device)),
                DispatchQueue::custom(WasdkQueue::new(queue)),
            ))
        })
    }

    fn is_surface_supported(&self, surface: &DispatchSurface) -> bool {
        // Every adapter of the engine's draws every view.
        surface.as_custom::<WasdkSurface>().is_some()
    }

    fn features(&self) -> Features {
        convert::features_from_wasi(&self.adapter.features())
    }

    fn limits(&self) -> Limits {
        convert::limits_from_wasi(&self.adapter.limits())
    }

    fn downlevel_capabilities(&self) -> DownlevelCapabilities {
        // WebGPU is fully compliant by definition.
        DownlevelCapabilities::default()
    }

    fn get_info(&self) -> AdapterInfo {
        convert::adapter_info_from_wasi(&self.adapter.info())
    }

    fn get_texture_format_features(&self, format: TextureFormat) -> TextureFormatFeatures {
        format.guaranteed_format_features(AdapterInterface::features(self))
    }

    fn get_presentation_timestamp(&self) -> PresentationTimestamp {
        PresentationTimestamp::INVALID_TIMESTAMP
    }

    fn cooperative_matrix_properties(&self) -> Vec<wgpu::wgt::CooperativeMatrixProperties> {
        Vec::new()
    }
}
