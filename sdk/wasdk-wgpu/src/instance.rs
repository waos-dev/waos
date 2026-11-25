//! The instance: the host's `gpu`, and the adapters it hands out.

use std::pin::Pin;
use std::sync::Arc;

use wasdk::webgpu::{self, Gpu, GpuRequestAdapterOptions};
use wgpu::custom::{
    DispatchAdapter, DispatchSurface, EnumerateAdapterFuture, InstanceInterface,
    RequestAdapterFuture,
};
use wgpu::rwh::WindowHandle;
use wgpu::{Backends, RequestAdapterError, WgslLanguageFeatures};

use crate::adapter::WasdkAdapter;
use crate::convert;
use crate::gap::wasi_gap;
use crate::surface::WasdkSurface;

/// The instance behind a [`wgpu::Instance`] from this backend:
/// `instance.as_custom::<WasdkInstance>()`.
#[derive(Debug)]
pub struct WasdkInstance {
    gpu: Arc<Gpu>,
    /// What the program asked for, kept for the error an adapter request reports.
    requested_backends: Backends,
}

impl WasdkInstance {
    /// The wasi `gpu` behind the instance.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The format the host draws its canvases in, which a view's surface offers.
    pub(crate) fn preferred_canvas_format(&self) -> wgpu::TextureFormat {
        convert::texture_format_from_wasi(self.gpu.get_preferred_canvas_format())
    }
}

/// wasi:webgpu is WebGPU, so it answers to wgpu's WebGPU backend bit.
const BACKEND: Backends = Backends::BROWSER_WEBGPU;

const WGSL_LANGUAGE_FEATURES: &[(WgslLanguageFeatures, &str)] = &[
    (
        WgslLanguageFeatures::ReadOnlyAndReadWriteStorageTextures,
        "readonly_and_readwrite_storage_textures",
    ),
    (
        WgslLanguageFeatures::Packed4x8IntegerDotProduct,
        "packed_4x8_integer_dot_product",
    ),
    (
        WgslLanguageFeatures::UnrestrictedPointerParameters,
        "unrestricted_pointer_parameters",
    ),
    (
        WgslLanguageFeatures::PointerCompositeAccess,
        "pointer_composite_access",
    ),
];

// `create_surface` is `unsafe` in the trait, and vouches for the window handle it is given.
#[allow(unsafe_code)]
impl InstanceInterface for WasdkInstance {
    fn new(desc: wgpu::InstanceDescriptor) -> Self {
        WasdkInstance {
            gpu: Arc::new(webgpu::get_gpu()),
            requested_backends: desc.backends,
        }
    }

    /// A surface on the view the target's window handle points at, holding a handle of its own
    /// to the view from here on; a target that is no window handle, or a handle that is no
    /// view's, is a gap: wgpu gives a backend of its own no way to make the `CreateSurfaceError`
    /// it would answer.
    unsafe fn create_surface(
        &self,
        target: wgpu::SurfaceTargetUnsafe,
    ) -> Result<DispatchSurface, wgpu::CreateSurfaceError> {
        let wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_window_handle, ..
        } = target
        else {
            wasi_gap("a surface from a target other than a window handle")
        };
        // SAFETY: wgpu's contract for the target is that its window is alive for the call.
        let handle = unsafe { WindowHandle::borrow_raw(raw_window_handle) };
        let Some(view) = wasdk::view_of_window_handle(handle) else {
            wasi_gap(format_args!(
                "a surface from a window handle other than a view's (a {raw_window_handle:?})"
            ))
        };
        let surface = WasdkSurface::new(view, self.preferred_canvas_format());
        Ok(DispatchSurface::custom(surface))
    }

    fn request_adapter(
        &self,
        options: &wgpu::RequestAdapterOptions<'_, '_>,
    ) -> Pin<Box<dyn RequestAdapterFuture>> {
        // Every adapter of the engine's draws every view, so a compatible surface narrows nothing.
        let requested_backends = self.requested_backends;
        let wasi_options = requested_backends
            .contains(BACKEND)
            .then(|| GpuRequestAdapterOptions {
                feature_level: None,
                power_preference: convert::power_preference_to_wasi(options.power_preference),
                force_fallback_adapter: Some(options.force_fallback_adapter),
                xr_compatible: None,
            });
        let gpu = Arc::clone(&self.gpu);
        Box::pin(async move {
            let adapter = match wasi_options {
                Some(wasi_options) => gpu.request_adapter(Some(wasi_options)).await,
                None => None,
            };
            adapter
                .map(|adapter| DispatchAdapter::custom(WasdkAdapter::new(adapter)))
                .ok_or_else(|| no_adapter(requested_backends))
        })
    }

    fn poll_all_devices(&self, _force_wait: bool) -> bool {
        // Devices make progress on their own; see `runtime`.
        true
    }

    fn wgsl_language_features(&self) -> WgslLanguageFeatures {
        let supported = self.gpu.wgsl_language_features();
        WGSL_LANGUAGE_FEATURES
            .iter()
            .filter(|(_, name)| supported.has(name))
            .fold(WgslLanguageFeatures::empty(), |features, (bit, _)| {
                features | bit.clone()
            })
    }

    fn enumerate_adapters(&self, backends: Backends) -> Pin<Box<dyn EnumerateAdapterFuture>> {
        if !backends.contains(BACKEND) {
            return Box::pin(async { Vec::new() });
        }
        let adapter = self.request_adapter(&wgpu::RequestAdapterOptions::default());
        Box::pin(async move { adapter.await.into_iter().collect() })
    }
}

fn no_adapter(requested_backends: Backends) -> RequestAdapterError {
    RequestAdapterError::NotFound {
        active_backends: BACKEND,
        requested_backends,
        supported_backends: BACKEND,
        no_fallback_backends: Backends::empty(),
        no_adapter_backends: BACKEND,
        incompatible_surface_backends: Backends::empty(),
    }
}
