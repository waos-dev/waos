//! Surfaces: wgpu's way to draw on a window, here on a program's view, as wgpu's browser backend
//! draws on a canvas. The window is a [`SharedView`], the program's view shared with the surfaces
//! made on it, and its window handle a `WebCanvasWindowHandle` pointing at it, as a browser's
//! points at a canvas: a component has no browser canvas for one to point at, and a view's id is
//! a `u64`, which the numbered `WebWindowHandle` cannot carry. A surface configures a canvas
//! context of the view's, hands out the context's texture each frame, and the frame's present
//! presents the view (`Queue::present`, in `queue`).
//!
//! The canvas is the size the surface is configured at: `configure` sets the view's canvas to its
//! width and height, so every frame's texture is the size wgpu tells for it, and a program
//! configures again as its view's size changes, as it would a window's surface anywhere.
//!
//! The invariant: the handle's pointer is read only in `create_surface`, while the window it was
//! taken from lends it; from there the surface, and each frame it hands out, hold the view through
//! its `Arc`, so nothing after needs the pointer.

use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::Arc;

use wasdk::views::View;
use wasdk::webgpu::{GpuCanvasConfiguration, GpuCanvasContext};
use wgpu::custom::{
    DispatchAdapter, DispatchDevice, DispatchSurfaceOutputDetail, DispatchTexture,
    SurfaceInterface, SurfaceOutputDetailInterface,
};
use wgpu::rwh::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    WebCanvasWindowHandle, WindowHandle,
};
use wgpu::{
    CompositeAlphaMode, PresentMode, SurfaceCapabilities, SurfaceColorSpaces,
    SurfaceFormatCapabilities, SurfaceStatus, TextureFormat, TextureUsages,
};

use crate::convert;
use crate::device::WasdkDevice;
use crate::gap::wasi_gap;
use crate::texture::WasdkTexture;

/// A program's view, shared by the program and the surfaces made on it: the window wgpu makes a
/// surface for, as an HTML canvas is on the web. It is the view's `Arc` with the window handle
/// Rust's orphan rule keeps off `Arc<View>`, so it dereferences to the view; every clone is the
/// same view.
///
/// ```ignore
/// let view = wasdk_wgpu::SharedView::new(view);
/// let surface = instance.create_surface(view.clone())?;
/// view.request_frame();
/// ```
#[derive(Clone, Debug)]
pub struct SharedView(Arc<View>);

impl SharedView {
    pub fn new(view: View) -> SharedView {
        SharedView(Arc::new(view))
    }
}

impl Deref for SharedView {
    type Target = View;

    fn deref(&self) -> &View {
        &self.0
    }
}

impl HasWindowHandle for SharedView {
    /// A `WebCanvas` handle pointing at this view.
    #[allow(unsafe_code)]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = WebCanvasWindowHandle::new(NonNull::from(self).cast());
        // SAFETY: the handle points at `self`, which the handle borrows for as long as it lives.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for SharedView {
    /// The web's display, which carries nothing: the view is reached through its window handle.
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::web())
    }
}

/// Where the view a window handle points at is: a `WebCanvas` handle's object, which in a
/// component is a [`SharedView`], since a component has no browser canvas. Any other handle is a
/// gap: wgpu gives a backend of its own no way to make the `CreateSurfaceError` it would answer.
pub(crate) fn shared_view_of(handle: RawWindowHandle) -> NonNull<SharedView> {
    match handle {
        RawWindowHandle::WebCanvas(handle) => handle.obj.cast(),
        other => wasi_gap(format_args!(
            "a surface from a window handle other than a view's (a {other:?})"
        )),
    }
}

/// What the view's canvas serves: its one format, presented at the view's frames, blended as
/// WebGPU's canvas blends, for drawing into, sampling and copying to and from.
pub(crate) fn capabilities(format: TextureFormat) -> SurfaceCapabilities {
    SurfaceCapabilities {
        formats: vec![format],
        format_capabilities: vec![SurfaceFormatCapabilities {
            format,
            color_spaces: SurfaceColorSpaces::SRGB,
        }],
        present_modes: vec![PresentMode::Fifo],
        alpha_modes: vec![
            CompositeAlphaMode::Opaque,
            CompositeAlphaMode::PreMultiplied,
        ],
        usages: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST,
    }
}

/// Refuses a present mode other than the view's: its frames come at the screen's refresh, which
/// is `Fifo`, and what an automatic mode falls back to.
fn check_present_mode(mode: PresentMode) {
    match mode {
        PresentMode::Fifo | PresentMode::AutoVsync | PresentMode::AutoNoVsync => {}
        other => wasi_gap(format_args!(
            "the present mode {other:?}: a view presents at its frames"
        )),
    }
}

/// The surface behind a [`wgpu::Surface`] on a view: `surface.as_custom::<WasdkSurface>()`.
#[derive(Debug)]
pub struct WasdkSurface {
    view: Arc<View>,
    context: GpuCanvasContext,
    /// The one format the view's canvas is drawn in.
    format: TextureFormat,
}

impl WasdkSurface {
    /// A surface on `view`, through a canvas context of its own, drawn in `format`.
    pub(crate) fn new(view: &SharedView, format: TextureFormat) -> WasdkSurface {
        WasdkSurface {
            view: Arc::clone(&view.0),
            context: view.context(),
            format,
        }
    }
}

impl SurfaceInterface for WasdkSurface {
    fn get_capabilities(&self, _adapter: &DispatchAdapter) -> SurfaceCapabilities {
        capabilities(self.format)
    }

    /// Sets the view's canvas to `width` by `height`, then configures its canvas context as
    /// WebGPU's canvas is configured; the present mode and the frame latency have no counterpart,
    /// a view presenting at its frames.
    fn configure(&self, device: &DispatchDevice, config: &wgpu::SurfaceConfiguration) {
        check_present_mode(config.present_mode);
        self.view.set_canvas_size(config.width, config.height);
        let (color_space, tone_mapping) = convert::color_space_to_wasi(config.color_space);
        let view_formats = config
            .view_formats
            .iter()
            .map(|format| convert::texture_format_to_wasi(*format))
            .collect();
        self.context.configure(&GpuCanvasConfiguration {
            device: WasdkDevice::of_dispatch(device).gpu_device(),
            format: convert::texture_format_to_wasi(config.format),
            usage: Some(convert::texture_usages_to_wasi(config.usage)),
            view_formats: Some(view_formats),
            color_space,
            tone_mapping,
            alpha_mode: Some(convert::alpha_mode_to_wasi(config.alpha_mode)),
        });
    }

    /// The canvas's texture for this frame, of the size configured. A lost device hands out an
    /// invalid texture, which the canvas cannot tell from a good one, so it is handed out as good:
    /// the program hears of the loss through the device.
    fn get_current_texture(
        &self,
    ) -> (
        Option<DispatchTexture>,
        SurfaceStatus,
        DispatchSurfaceOutputDetail,
    ) {
        let texture = self.context.get_current_texture();
        let frame = WasdkSurfaceOutputDetail {
            view: Arc::clone(&self.view),
        };
        (
            Some(DispatchTexture::custom(WasdkTexture::new(texture))),
            SurfaceStatus::Good,
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
    use wgpu::rwh::{AppKitWindowHandle, WebWindowHandle};

    use super::*;

    #[test]
    fn a_view_serves_its_canvas_format_at_its_frames() {
        let capabilities = capabilities(TextureFormat::Bgra8Unorm);
        assert_eq!(capabilities.formats, [TextureFormat::Bgra8Unorm]);
        assert_eq!(
            capabilities.color_spaces(TextureFormat::Bgra8Unorm),
            SurfaceColorSpaces::SRGB
        );
        assert_eq!(capabilities.present_modes, [PresentMode::Fifo]);
        assert_eq!(capabilities.alpha_modes[0], CompositeAlphaMode::Opaque);
        assert!(
            capabilities
                .usages
                .contains(TextureUsages::RENDER_ATTACHMENT)
        );
    }

    #[test]
    fn an_automatic_present_mode_is_the_views_frames() {
        check_present_mode(PresentMode::AutoNoVsync);
        check_present_mode(PresentMode::Fifo);
    }

    #[test]
    #[should_panic(expected = "Mailbox")]
    fn a_present_mode_other_than_the_views_frames_is_a_gap() {
        check_present_mode(PresentMode::Mailbox);
    }

    /// `instance.create_surface(view.clone())` takes it, and Bevy's `RawHandleWrapper` its two
    /// handles.
    #[test]
    fn a_shared_view_is_a_window_wgpu_makes_a_surface_for() {
        fn surface_target<T: Into<wgpu::SurfaceTarget<'static>>>() {}
        fn window<T: wgpu::WindowHandle + HasDisplayHandle>() {}
        surface_target::<SharedView>();
        window::<SharedView>();
    }

    #[test]
    fn a_web_canvas_handle_points_at_the_shared_view() {
        let pointer = NonNull::<SharedView>::dangling();
        let handle = WebCanvasWindowHandle::new(pointer.cast());
        assert_eq!(shared_view_of(handle.into()), pointer);
    }

    #[test]
    #[should_panic(expected = "other than a view's")]
    fn a_numbered_web_handle_is_not_a_view() {
        shared_view_of(WebWindowHandle::new(1).into());
    }

    #[test]
    #[should_panic(expected = "other than a view's")]
    fn a_native_window_is_not_a_view() {
        let window = NonNull::<u8>::dangling().cast();
        shared_view_of(AppKitWindowHandle::new(window).into());
    }
}
