//! Where a frame is drawn: the view's surface, with the multisampled colour and depth textures
//! the scene is drawn into before it is resolved onto the surface's own.
//!
//! The invariant: the three are always one size, the one last configured, so they are made again
//! together at each change of the view's size.

use wasdk_wgpu::SharedView;

use crate::gpu::Gpu;

/// The samples a pixel of the scene is drawn with.
pub const SAMPLES: u32 = 4;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct Canvas {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
}

impl Canvas {
    /// A surface on `view`, configured at the view's size in the format it prefers.
    pub fn new(gpu: &Gpu, view: &SharedView) -> Result<Canvas, String> {
        let surface = gpu
            .instance
            .create_surface(view.clone())
            .map_err(|error| format!("no surface on the view: {error}"))?;
        let metrics = view.metrics();
        let config = surface
            .get_default_config(&gpu.adapter, metrics.width.max(1), metrics.height.max(1))
            .ok_or("the view's surface draws in no format")?;
        surface.configure(&gpu.device, &config);
        Ok(Canvas {
            colour: multisampled(&gpu.device, &config, config.format),
            depth: multisampled(&gpu.device, &config, DEPTH_FORMAT),
            surface,
            config,
        })
    }

    /// Draws at `width` by `height` physical pixels from the next frame on. A view of no size,
    /// as a window being minimized may report, keeps the last one.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 || [width, height] == self.size() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(device, &self.config);
        self.colour = multisampled(device, &self.config, self.config.format);
        self.depth = multisampled(device, &self.config, DEPTH_FORMAT);
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    /// The surface's texture for this frame; none when it has none to give.
    pub fn frame(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            _ => None,
        }
    }

    /// The multisampled colour the scene is drawn into.
    pub fn colour(&self) -> &wgpu::TextureView {
        &self.colour
    }

    pub fn depth(&self) -> &wgpu::TextureView {
        &self.depth
    }
}

fn multisampled(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene target"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: SAMPLES,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
