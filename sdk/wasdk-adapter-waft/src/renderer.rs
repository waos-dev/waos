//! What every view of the program draws with: one device and one valo context, so the glyph atlas
//! and the pipelines are built once however many views there are. Each view draws on a surface of
//! its own, which the renderer makes for it, keeps configured at the view's size, draws each frame
//! into and presents on the view. The context's glyphs come through the engine's glyph calls
//! ([`ProgramGlyphs`]).

use std::cell::RefCell;
use std::sync::Arc;

use waft_embedder::Picture;
use waft_embedder::{HardwareBuffer, VideoFrame};
use wasdk::views::View;

use crate::fonts::ProgramGlyphs;
use crate::gpu_interop;

/// What valo draws into: a render attachment it can also copy from, for backdrop filters and
/// advanced blends that read the target back mid-frame.
const SURFACE_USAGE: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::COPY_SRC);

/// The instance views' surfaces are made on, the device, its queue, which presents each view's
/// frames, and the valo context every view of the program shares.
pub(crate) struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    context: RefCell<valo::Context>,
}

impl Renderer {
    /// A renderer on the engine's WebGPU.
    ///
    /// # Panics
    ///
    /// When the engine gives no adapter or no device: a program that draws has nothing to draw
    /// with.
    pub(crate) async fn new() -> Renderer {
        let instance = wasdk_wgpu::instance();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("the engine gives a WebGPU adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("waft.wasdk"),
                ..Default::default()
            })
            .await
            .expect("the engine gives a WebGPU device");
        let mut context = valo::Context::with_glyph_raster(
            device.clone(),
            queue.clone(),
            Box::new(ProgramGlyphs::new()),
        );
        // As Waft's own hosts draw: a character no font covers shows nothing, not a box.
        context.set_hide_missing_glyphs(true);
        Renderer {
            instance,
            adapter,
            device,
            queue,
            context: RefCell::new(context),
        }
    }

    /// A surface on `view`, configured for the renderer's device at the view's size, in the
    /// format the view's canvas is drawn in.
    ///
    /// # Panics
    ///
    /// When the engine makes no surface on the view: a view is the one window a program has.
    pub(crate) fn surface(&self, view: &Arc<View>) -> wgpu::Surface<'static> {
        let surface = self
            .instance
            .create_surface(view.clone())
            .expect("the engine makes a surface on a view");
        let metrics = view.metrics();
        let format = surface.get_capabilities(&self.adapter).formats[0];
        let configuration = wgpu::SurfaceConfiguration {
            usage: SURFACE_USAGE,
            format,
            width: metrics.width,
            height: metrics.height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&self.device, &configuration);
        surface
    }

    /// Draws `picture` from `clear` into the frame `surface` hands out at `size`, the view's now,
    /// and answers the frame for its view to present; none when the surface hands out none.
    pub(crate) fn draw(
        &self,
        picture: &Picture,
        surface: &wgpu::Surface<'_>,
        size: [u32; 2],
        clear: valo::Color,
    ) -> Option<wgpu::SurfaceTexture> {
        let frame = self.frame_at(surface, size)?;
        self.render(picture, &frame.texture, clear);
        Some(frame)
    }

    /// The frame `surface` hands out, configured first at `size` unless it is configured at it:
    /// the view's canvas is the size its surface is configured at, so configuring at the view's
    /// size now draws the picture at it.
    fn frame_at(
        &self,
        surface: &wgpu::Surface<'_>,
        size: [u32; 2],
    ) -> Option<wgpu::SurfaceTexture> {
        self.configure_at(surface, size);
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            _ => None,
        }
    }

    /// Draws `picture` from `clear` into `texture`.
    fn render(&self, picture: &Picture, texture: &wgpu::Texture, clear: valo::Color) {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.context.borrow_mut().render(
            picture,
            &valo::RenderTarget {
                view: &view,
                texture,
                format: texture.format(),
                // The surface is configured at the view's size now, which may already be newer
                // than the one this picture was laid out for; the target is the texture.
                size: [texture.width(), texture.height()],
                clear: Some(clear),
            },
        );
    }

    /// Presents `frame` on its view, with the layers the view holds beneath it.
    pub(crate) fn present(&self, frame: wgpu::SurfaceTexture) {
        self.queue.present(frame);
    }

    /// Configures `surface` at `size` unless it is configured at it.
    fn configure_at(&self, surface: &wgpu::Surface<'_>, [width, height]: [u32; 2]) {
        let configuration = surface
            .get_configuration()
            .expect("a view's surface is configured as it is made");
        if [configuration.width, configuration.height] != [width, height] {
            let resized = wgpu::SurfaceConfiguration {
                width,
                height,
                ..configuration
            };
            surface.configure(&self.device, &resized);
        }
    }

    /// A video's `frame` as an image on the renderer's device, as its hardware buffer is.
    pub(crate) fn import_frame(&self, frame: &dyn VideoFrame) -> Option<valo::Image> {
        self.import_buffer(&*frame.hardware_buffer(), false)
    }

    /// A hardware `buffer` as an image on the renderer's device: the texture imported of it there,
    /// which holds the buffer, and a frame's buffer its frame, until it is dropped, or, with
    /// `mipmaps`, a copy of it with its mip levels made on the GPU; none for a buffer the WIT did
    /// not hand out, or one valo cannot sample.
    pub(crate) fn import_buffer(
        &self,
        buffer: &dyn HardwareBuffer,
        mipmaps: bool,
    ) -> Option<valo::Image> {
        let texture = gpu_interop::import_texture(buffer, &self.device)?;
        self.context
            .borrow()
            .image_context()
            .import_texture(texture, mipmaps)
            .ok()
    }

    /// Pixels the program already holds, uploaded as an image on the renderer's device, with its
    /// mip levels where `mipmaps`.
    pub(crate) fn import_pixels(
        &self,
        pixels: valo::PixelBuffer,
        mipmaps: bool,
    ) -> Option<valo::Image> {
        self.context
            .borrow()
            .image_context()
            .upload_pixels(pixels, mipmaps)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valo_may_read_back_what_it_draws_into() {
        assert!(SURFACE_USAGE.contains(wgpu::TextureUsages::RENDER_ATTACHMENT));
        assert!(SURFACE_USAGE.contains(wgpu::TextureUsages::COPY_SRC));
    }
}
