//! The instance: the engine's adapters, and surfaces on views.
//!
//! `wasdk:gpu` has no instance object: each program has one, the engine's, whose functions are
//! the interface's own (`request-adapter`, `enumerate-adapters`, `wgsl-language-features`). The
//! engine picks the backends; the backends a program asks for only filter the adapters it is given.

use std::pin::Pin;

use wasdk::gpu::{self, Adapter, RequestAdapterOptions};
use wgpu::custom::{
    DispatchAdapter, DispatchSurface, EnumerateAdapterFuture, InstanceInterface,
    RequestAdapterFuture,
};
use wgpu::rwh::WindowHandle;
use wgpu::{Backends, RequestAdapterError, WgslLanguageFeatures};

use crate::adapter::WasdkAdapter;
use crate::convert;
use crate::left_out::left_out;
use crate::surface::WasdkSurface;

/// The instance behind a [`wgpu::Instance`] from this backend:
/// `instance.as_custom::<WasdkInstance>()`.
#[derive(Debug)]
pub struct WasdkInstance {
    /// The backends the program asked for, which filter the engine's adapters.
    requested_backends: Backends,
}

// `create_surface` is `unsafe` in the trait, and vouches for the window handle it is given.
#[allow(unsafe_code)]
impl InstanceInterface for WasdkInstance {
    fn new(desc: wgpu::InstanceDescriptor) -> Self {
        WasdkInstance {
            requested_backends: desc.backends,
        }
    }

    /// The surface of the view the target's window handle names, holding a handle of its own
    /// to the view from here on. Only a view makes a surface: a target that is no window handle,
    /// or a handle that is no view's, is left out, since wgpu gives a backend of its own no way to
    /// make the `CreateSurfaceError` it would answer.
    unsafe fn create_surface(
        &self,
        target: wgpu::SurfaceTargetUnsafe,
    ) -> Result<DispatchSurface, wgpu::CreateSurfaceError> {
        let wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_window_handle, ..
        } = target
        else {
            left_out("a surface from a target other than a window handle")
        };
        // SAFETY: wgpu's contract for the target is that its window is alive for the call.
        let handle = unsafe { WindowHandle::borrow_raw(raw_window_handle) };
        let Some(view) = wasdk::view_of_window_handle(handle) else {
            left_out(format_args!(
                "a surface from a window handle other than a view's (a {raw_window_handle:?})"
            ))
        };
        Ok(DispatchSurface::custom(WasdkSurface::new(view)))
    }

    fn request_adapter(
        &self,
        options: &wgpu::RequestAdapterOptions<'_, '_>,
    ) -> Pin<Box<dyn RequestAdapterFuture>> {
        let requested_backends = self.requested_backends;
        let power_preference = convert::power_preference_to_wit(options.power_preference);
        let force_fallback_adapter = options.force_fallback_adapter;
        let compatible_surface = options
            .compatible_surface
            .map(|surface| WasdkSurface::of(surface).shared_surface());
        Box::pin(async move {
            let options = RequestAdapterOptions {
                power_preference,
                force_fallback_adapter,
                compatible_surface: compatible_surface.as_deref(),
            };
            gpu::request_adapter(options)
                .await
                .filter(|adapter| is_requested(adapter, requested_backends))
                .map(|adapter| DispatchAdapter::custom(WasdkAdapter::new(adapter)))
                .ok_or_else(|| no_adapter(requested_backends))
        })
    }

    fn poll_all_devices(&self, _force_wait: bool) -> bool {
        // A program polls its own devices; see `runtime`.
        true
    }

    fn wgsl_language_features(&self) -> WgslLanguageFeatures {
        convert::wgsl_language_features_from_wit(gpu::wgsl_language_features())
    }

    fn enumerate_adapters(&self, backends: Backends) -> Pin<Box<dyn EnumerateAdapterFuture>> {
        let backends = backends & self.requested_backends;
        Box::pin(async move {
            (gpu::enumerate_adapters().await.into_iter())
                .filter(|adapter| is_requested(adapter, backends))
                .map(|adapter| DispatchAdapter::custom(WasdkAdapter::new(adapter)))
                .collect()
        })
    }
}

/// Whether `adapter` is reached through one of `backends`.
fn is_requested(adapter: &Adapter, backends: Backends) -> bool {
    let backend = convert::backend_from_wit(adapter.info().backend);
    backends.contains(Backends::from(backend))
}

fn no_adapter(requested_backends: Backends) -> RequestAdapterError {
    RequestAdapterError::NotFound {
        active_backends: requested_backends,
        requested_backends,
        supported_backends: Backends::all(),
        no_fallback_backends: Backends::empty(),
        no_adapter_backends: requested_backends,
        incompatible_surface_backends: Backends::empty(),
    }
}
