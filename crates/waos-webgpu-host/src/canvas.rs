//! `gpu-canvas-context`: WebGPU's canvas model over a target the platform owns.
//!
//! The platform creates the target and keeps its size current; the program configures the
//! context against its device and takes one texture per frame, as it would from an HTML
//! canvas. The target may keep the frame's texture until the frame is presented, as WebGPU's
//! canvas keeps its current texture, so what the target does at present finds it whether or not
//! the program still holds it. A window's drawable area, drawn through its swapchain, is one such target and the
//! one this module supplies; a platform that composites a view itself brings another.
//! Presenting is the platform's, not the context's: WebGPU presents a canvas implicitly at
//! the end of a task, and a platform without that notion offers an explicit call on the view
//! the target belongs to.

use std::sync::{Arc, Mutex};

use wasmtime::{bail, component::Resource};

use crate::wasi::webgpu::webgpu;
use crate::{Device, GpuObject, TextureAttributes, WasiWebGpuCtx};

/// What a canvas context draws into: a target at whatever size the platform keeps for it,
/// which hands out one texture per frame.
pub trait CanvasSurface: Send + Sync {
    /// The target's size in physical pixels, as it is now.
    fn size(&self) -> (u32, u32);

    /// Makes the target ready to hand out frames of `configuration`'s size and format, on
    /// `device`. Called again whenever the size no longer fits.
    fn configure(
        &self,
        instance: &wgpu_core::global::Global,
        device: &Device,
        configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
    ) -> wasmtime::Result<()>;

    /// The texture for the frame being drawn, on the device the target was configured with. It
    /// goes back to `instance` once the program and the target have both let go of it.
    fn current_texture(
        &self,
        instance: &Arc<wgpu_core::global::Global>,
    ) -> wasmtime::Result<Arc<GpuObject<wgpu_core::id::TextureId>>>;
}

/// A [`CanvasSurface`] over a wgpu surface: the platform's window or screen, drawn through
/// its swapchain. The platform supplies the size, because only it knows how the surface is
/// laid out; the swapchain follows.
///
/// The canvas model always hands out a texture, and a swapchain has none to give while its
/// window is covered, minimised or behind a locked screen. Then the program draws into a
/// texture of its own and the frame is not presented: what the window would have shown
/// nobody could see.
pub struct SwapchainSurface {
    surface: wgpu_core::id::SurfaceId,
    size: Box<dyn Fn() -> (u32, u32) + Send + Sync>,
    /// What `configure` was last given, so the swapchain can be rebuilt without the
    /// program's involvement when the window system invalidates it, and whether the frame
    /// being drawn is the swapchain's own.
    configured: Mutex<Option<Configured>>,
}

/// A swapchain as `configure` left it.
struct Configured {
    device: wgpu_core::id::DeviceId,
    surface: wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
    /// Whether `current_texture` last gave the swapchain's texture, which `present` puts
    /// on screen, rather than a texture of the program's own, which it does not.
    presentable: bool,
}

impl SwapchainSurface {
    /// A target over `surface`, sized by `size` whenever the size is asked for.
    pub fn new(
        surface: wgpu_core::id::SurfaceId,
        size: impl Fn() -> (u32, u32) + Send + Sync + 'static,
    ) -> SwapchainSurface {
        SwapchainSurface {
            surface,
            size: Box::new(size),
            configured: Mutex::new(None),
        }
    }

    /// The wgpu surface, for the platform to drop.
    pub fn surface_id(&self) -> wgpu_core::id::SurfaceId {
        self.surface
    }

    /// Puts the frame last taken on screen, when it was the swapchain's; a frame drawn
    /// into a texture of the program's own, because the window had none to give, is let
    /// go.
    pub fn present(&self, instance: &wgpu_core::global::Global) -> wasmtime::Result<()> {
        let mut configured = self.configured.lock().unwrap();
        let Some(configured) = configured.as_mut() else {
            bail!("the swapchain has not been configured")
        };
        if !std::mem::replace(&mut configured.presentable, false) {
            return Ok(());
        }
        instance.surface_present(self.surface)?;
        Ok(())
    }

    /// The swapchain's texture for the frame, or one of the program's own when it has none.
    fn swapchain_or_offscreen_texture(
        &self,
        instance: &wgpu_core::global::Global,
    ) -> wasmtime::Result<wgpu_core::id::TextureId> {
        // A swapchain has no texture to give while the window is covered, being resized, or
        // just reconfigured; wgpu says which. An outdated or lost swapchain is rebuilt at the
        // surface's current size and asked once more; a covered window, or one whose
        // compositor is late, gets a texture of the program's own and no present.
        let mut configured = self.configured.lock().unwrap();
        let Some(configured) = configured.as_mut() else {
            bail!("the swapchain has not been configured")
        };
        configured.presentable = false;
        for attempt in 0..2 {
            let output = match instance.surface_get_current_texture(self.surface, None) {
                Ok(output) => output,
                Err(error) => bail!("{error:#?}"),
            };
            if let Some(texture) = output.texture {
                configured.presentable = true;
                return Ok(texture);
            }
            let rebuild = matches!(
                output.status,
                wgpu_types::SurfaceStatus::Outdated | wgpu_types::SurfaceStatus::Lost
            );
            if !rebuild || attempt == 1 {
                break;
            }
            configure_swapchain(instance, self.surface, configured)?;
        }
        SwapchainSurface::offscreen_texture(instance, configured)
    }

    /// A texture of the program's own, of the swapchain's size and format, for a frame the
    /// window cannot show.
    fn offscreen_texture(
        instance: &wgpu_core::global::Global,
        configured: &Configured,
    ) -> wasmtime::Result<wgpu_core::id::TextureId> {
        let descriptor = wgpu_types::TextureDescriptor {
            label: Some(std::borrow::Cow::Borrowed("canvas, skipped frame")),
            size: wgpu_types::Extent3d {
                width: configured.surface.width,
                height: configured.surface.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu_types::TextureDimension::D2,
            format: configured.surface.format,
            usage: configured.surface.usage,
            view_formats: configured.surface.view_formats.clone(),
        };
        let (texture, error) = instance.device_create_texture(configured.device, &descriptor, None);
        if let Some(error) = error {
            bail!("{error:#?}")
        }
        Ok(texture)
    }
}

fn configure_swapchain(
    instance: &wgpu_core::global::Global,
    surface: wgpu_core::id::SurfaceId,
    configured: &Configured,
) -> wasmtime::Result<()> {
    if let Some(error) = instance.surface_configure(surface, configured.device, &configured.surface)
    {
        bail!("{error:#?}")
    }
    Ok(())
}

impl CanvasSurface for SwapchainSurface {
    fn size(&self) -> (u32, u32) {
        (self.size)()
    }

    fn configure(
        &self,
        instance: &wgpu_core::global::Global,
        device: &Device,
        configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
    ) -> wasmtime::Result<()> {
        let configured = Configured {
            device: device.device_id(),
            surface: configuration.clone(),
            presentable: false,
        };
        configure_swapchain(instance, self.surface, &configured)?;
        *self.configured.lock().unwrap() = Some(configured);
        Ok(())
    }

    fn current_texture(
        &self,
        instance: &Arc<wgpu_core::global::Global>,
    ) -> wasmtime::Result<Arc<GpuObject<wgpu_core::id::TextureId>>> {
        let texture = self.swapchain_or_offscreen_texture(instance)?;
        Ok(Arc::new(GpuObject::new(texture, instance)))
    }
}

/// A `gpu-canvas-context`: a target and the configuration the program gave it.
pub struct CanvasContext {
    surface: Arc<dyn CanvasSurface>,
    configuration: Option<Configuration>,
}

impl CanvasContext {
    pub fn new(surface: Arc<dyn CanvasSurface>) -> CanvasContext {
        CanvasContext {
            surface,
            configuration: None,
        }
    }
}

/// What `configure` was given, and what it became for wgpu. Kept so the target can be
/// rebuilt at its current size without the program's involvement, and so
/// `get-configuration` can answer.
struct Configuration {
    device: Device,
    format: webgpu::GpuTextureFormat,
    usage: Option<webgpu::GpuTextureUsage>,
    view_formats: Option<Vec<webgpu::GpuTextureFormat>>,
    color_space: Option<webgpu::PredefinedColorSpace>,
    tone_mapping: Option<webgpu::GpuCanvasToneMapping>,
    alpha_mode: Option<webgpu::GpuCanvasAlphaMode>,
    surface: wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
}

impl Configuration {
    /// Configures the target at its current size.
    fn apply(
        &mut self,
        instance: &wgpu_core::global::Global,
        surface: &dyn CanvasSurface,
    ) -> wasmtime::Result<()> {
        (self.surface.width, self.surface.height) = surface.size();
        surface.configure(instance, &self.device, &self.surface)
    }

    fn fits(&self, surface: &dyn CanvasSurface) -> bool {
        (self.surface.width, self.surface.height) == surface.size()
    }
}

impl<'a> webgpu::HostGpuCanvasContext for WasiWebGpuCtx<'a> {
    fn configure(
        &mut self,
        context: Resource<webgpu::GpuCanvasContext>,
        configuration: webgpu::GpuCanvasConfiguration,
    ) -> wasmtime::Result<()> {
        let device = self.table.get(&configuration.device)?.clone();
        let context = self.table.get_mut(&context)?;
        let surface = wgpu_types::SurfaceConfiguration {
            // Present in WebGPU, with its defaults: https://www.w3.org/TR/webgpu/#dictdef-gpucanvasconfiguration
            format: configuration.format.into(),
            usage: configuration
                .usage
                .unwrap_or(webgpu::GpuTextureUsage::RENDER_ATTACHMENT)
                .try_into()?,
            view_formats: configuration
                .view_formats
                .iter()
                .flatten()
                .map(|format| (*format).into())
                .collect(),
            alpha_mode: configuration
                .alpha_mode
                .unwrap_or(webgpu::GpuCanvasAlphaMode::Opaque)
                .into(),
            // WebGPU's `color-space` has no wgpu counterpart; `Auto` leaves the color space to
            // the backend, which picks sRGB for every format a canvas can be configured with.
            color_space: wgpu_types::SurfaceColorSpace::Auto,
            // Not present in WebGPU.
            width: 0,
            height: 0,
            present_mode: wgpu_types::PresentMode::default(),
            desired_maximum_frame_latency: 2,
        };
        let mut applied = Configuration {
            device,
            format: configuration.format,
            usage: configuration.usage,
            view_formats: configuration.view_formats,
            color_space: configuration.color_space,
            tone_mapping: configuration.tone_mapping,
            alpha_mode: configuration.alpha_mode,
            surface,
        };
        applied.apply(self.instance, &*context.surface)?;
        context.configuration = Some(applied);
        Ok(())
    }

    fn get_configuration(
        &mut self,
        context: Resource<webgpu::GpuCanvasContext>,
    ) -> wasmtime::Result<Option<webgpu::GpuCanvasConfigurationOwned>> {
        let Some(configuration) = &self.table.get(&context)?.configuration else {
            return Ok(None);
        };
        let device = configuration.device.clone();
        let mut owned = webgpu::GpuCanvasConfigurationOwned {
            device: Resource::new_own(0),
            format: configuration.format,
            usage: configuration.usage,
            view_formats: configuration.view_formats.clone(),
            color_space: configuration.color_space,
            tone_mapping: configuration.tone_mapping,
            alpha_mode: configuration.alpha_mode,
        };
        owned.device = self.table.push(device)?;
        Ok(Some(owned))
    }

    fn unconfigure(&mut self, context: Resource<webgpu::GpuCanvasContext>) -> wasmtime::Result<()> {
        self.table.get_mut(&context)?.configuration = None;
        Ok(())
    }

    fn get_current_texture(
        &mut self,
        context: Resource<webgpu::GpuCanvasContext>,
    ) -> wasmtime::Result<Resource<webgpu::GpuTexture>> {
        let CanvasContext {
            surface,
            configuration,
        } = self.table.get_mut(&context)?;
        let Some(configuration) = configuration else {
            bail!("the canvas context is not configured")
        };
        // The target follows the platform's size, as a canvas does: a resize needs nothing
        // from the program.
        if !configuration.fits(&**surface) {
            configuration.apply(self.instance, &**surface)?;
        }
        let texture = surface.current_texture(self.instance)?;
        // https://www.w3.org/TR/webgpu/#abstract-opdef-replace-the-drawing-buffer
        let attributes = TextureAttributes {
            size: wgpu_types::Extent3d {
                width: configuration.surface.width,
                height: configuration.surface.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: webgpu::GpuTextureDimension::D2,
            format: configuration.format,
            usage: configuration
                .usage
                .unwrap_or(webgpu::GpuTextureUsage::RENDER_ATTACHMENT),
            texture_binding_view_dimension: None,
        };
        // A target only gives back a texture on the device it was configured with.
        let texture = configuration.device.connect_texture(texture, attributes);
        Ok(self.table.push(texture)?)
    }

    fn drop(&mut self, context: Resource<webgpu::GpuCanvasContext>) -> wasmtime::Result<()> {
        self.table.delete(context)?;
        Ok(())
    }
}
