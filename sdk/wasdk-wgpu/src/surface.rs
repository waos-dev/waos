//! Surfaces: wgpu's way to draw on a window, here on a program's view. The window is the view
//! itself, or any window over one: `wasdk` gives a view its window handle, and the view a handle
//! points at (`wasdk::view_of_window_handle`), which is how a surface finds its view
//! (`instance`). Each call maps to the view's surface's (`view.surface()`): its capabilities, its
//! configuration, its texture each frame; and the frame's present presents the view, with the
//! layers it holds beneath it (`Queue::present`, in `queue`).
//!
//! The invariant: a surface holds a handle of its own to the view, taken as it is made, and each
//! frame it hands out holds the view through it, so the window the surface was made from may go
//! first.

use std::sync::Arc;

use wasdk::gpu::{CurrentSurfaceTexture, Surface};
use wasdk::views::View;
use wgpu::custom::{
    DispatchAdapter, DispatchDevice, DispatchSurfaceOutputDetail, DispatchTexture,
    SurfaceInterface, SurfaceOutputDetailInterface,
};
use wgpu::{DisplayHdrInfo, SurfaceCapabilities, SurfaceStatus};

use crate::adapter::WasdkAdapter;
use crate::convert;
use crate::device::WasdkDevice;
use crate::texture::WasdkTexture;

/// The surface behind a [`wgpu::Surface`] on a view: `surface.as_custom::<WasdkSurface>()`.
#[derive(Debug)]
pub struct WasdkSurface {
    view: Arc<View>,
    /// The view's surface, shared with the adapter requests that name it.
    surface: Arc<Surface>,
}

downcast!(WasdkSurface: dispatch wgpu::custom::DispatchSurface);

impl WasdkSurface {
    /// This backend's surface behind `surface`; one from another wgpu backend is a programming
    /// error.
    pub(crate) fn of<'a>(surface: &'a wgpu::Surface<'_>) -> &'a Self {
        (surface.as_custom::<Self>())
            .expect("wasdk-wgpu: a wgpu::Surface from another wgpu backend")
    }

    /// The surface of `view`.
    pub(crate) fn new(view: View) -> WasdkSurface {
        WasdkSurface {
            surface: Arc::new(view.surface()),
            view: Arc::new(view),
        }
    }

    /// The view's surface behind the wgpu one.
    pub fn wit_surface(&self) -> &Surface {
        &self.surface
    }

    /// The view's surface, for a request that outlives this borrow.
    pub(crate) fn shared_surface(&self) -> Arc<Surface> {
        Arc::clone(&self.surface)
    }
}

impl SurfaceInterface for WasdkSurface {
    fn get_capabilities(&self, adapter: &DispatchAdapter) -> SurfaceCapabilities {
        let adapter = WasdkAdapter::of_dispatch(adapter).wit_adapter();
        convert::surface_capabilities_from_wit(self.surface.capabilities(adapter))
    }

    fn display_hdr_info(&self, adapter: &DispatchAdapter) -> DisplayHdrInfo {
        let adapter = WasdkAdapter::of_dispatch(adapter).wit_adapter();
        convert::display_hdr_info_from_wit(self.surface.hdr_info(adapter))
    }

    fn configure(&self, device: &DispatchDevice, config: &wgpu::SurfaceConfiguration) {
        self.surface.configure(
            WasdkDevice::of_dispatch(device).wit_device(),
            &convert::surface_configuration_to_wit(config),
        );
    }

    /// The surface's texture for this frame, or why there is none: `Occluded` while the view is
    /// not on screen.
    fn get_current_texture(
        &self,
    ) -> (
        Option<DispatchTexture>,
        SurfaceStatus,
        DispatchSurfaceOutputDetail,
    ) {
        let (texture, status) = match self.surface.current_texture() {
            CurrentSurfaceTexture::Success(texture) => (Some(texture), SurfaceStatus::Good),
            CurrentSurfaceTexture::Suboptimal(texture) => {
                (Some(texture), SurfaceStatus::Suboptimal)
            }
            CurrentSurfaceTexture::Timeout => (None, SurfaceStatus::Timeout),
            CurrentSurfaceTexture::Occluded => (None, SurfaceStatus::Occluded),
            CurrentSurfaceTexture::Outdated => (None, SurfaceStatus::Outdated),
            CurrentSurfaceTexture::Lost => (None, SurfaceStatus::Lost),
            CurrentSurfaceTexture::Validation => (None, SurfaceStatus::Validation),
        };
        let frame = WasdkSurfaceOutputDetail {
            view: Arc::clone(&self.view),
        };
        (
            texture.map(|texture| DispatchTexture::custom(WasdkTexture::new(texture))),
            status,
            DispatchSurfaceOutputDetail::custom(frame),
        )
    }
}

/// A frame a surface handed out: the view it is presented on.
#[derive(Debug)]
pub struct WasdkSurfaceOutputDetail {
    view: Arc<View>,
}

downcast!(WasdkSurfaceOutputDetail: dispatch DispatchSurfaceOutputDetail);

impl WasdkSurfaceOutputDetail {
    /// Presents the frame on its view, with the layers the view holds beneath it.
    pub(crate) fn present(&self) {
        self.view.present();
    }
}

impl SurfaceOutputDetailInterface for WasdkSurfaceOutputDetail {
    /// A frame dropped without a present is not presented: the view presents only when asked, and
    /// the next frame taken replaces it.
    fn texture_discard(&self) {}

    fn texture_release(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `instance.create_surface(view.clone())` takes it, and Bevy's `RawHandleWrapper` its two
    /// handles.
    #[test]
    fn a_shared_view_is_a_window_wgpu_makes_a_surface_for() {
        fn surface_target<T: Into<wgpu::SurfaceTarget<'static>>>() {}
        fn window<T: wgpu::WindowHandle + wgpu::rwh::HasDisplayHandle>() {}
        surface_target::<Arc<View>>();
        window::<Arc<View>>();
    }
}
