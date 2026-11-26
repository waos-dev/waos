//! The surface: where a view's frames come from, as wgpu's surface is. The embedder brings the
//! target behind it, the [`SurfaceTarget`], which says what it can be configured with, makes
//! itself ready for a configuration, hands out one texture per frame, and takes one back
//! unpresented; presenting is the view's, which owns the frame's timing and its layers.
//!
//! The invariant: a frame's texture is handed to the program on the device the surface was last
//! configured with, its errors, a frame refused among them, that device's.

use std::sync::{Arc, Mutex, Weak};

use wasmtime::component::Resource;
use wgpu_core::global::Global;
use wgpu_core::id::{AdapterId, TextureId};
use wgpu_core::present::{ConfigureSurfaceError, SurfaceError};

use crate::adapters::Adapter;
use crate::devices::Device;
use crate::textures::Texture;
use crate::wasdk::gpu::gpu::{
    self, CurrentSurfaceTexture, DisplayHdrInfo, SurfaceCapabilities, SurfaceConfiguration,
};
use crate::{GpuCtx, GpuObject};

/// What a surface draws into: a target the embedder keeps, at the size each configuration
/// gives, handing out one texture per frame. What fails in the embedder, as a system refusing
/// memory, is an `Err` of the outer result and traps the program; what wgpu would refuse of the
/// program is the inner one, which reaches the device's errors.
pub trait SurfaceTarget: Send + Sync {
    /// What the target can be configured with on `adapter` of `instance`; no formats where the
    /// adapter cannot draw into it.
    fn capabilities(
        &self,
        instance: &Global,
        adapter: AdapterId,
    ) -> wgpu_types::SurfaceCapabilities;

    /// Makes the target hand out frames as `configuration` says, drawn on `device`, or refuses
    /// the configuration as wgpu-core's `surface_configure` would.
    fn configure(
        &self,
        device: &Device,
        configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
    ) -> wasmtime::Result<Result<(), ConfigureSurfaceError>>;

    /// The frame to draw, or how the target stands where it has none; asked only once the
    /// target is configured. A frame asked while the last one handed out is neither presented
    /// nor discarded is refused, as wgpu-core refuses it (`SurfaceError::AlreadyAcquired`).
    fn current_texture(&self) -> wasmtime::Result<Result<SurfaceFrame, SurfaceError>>;

    /// Takes back the frame last handed out, unpresented, as wgpu-core's
    /// `surface_texture_discard` does: the next present shows nothing new. Nothing where no
    /// frame is held.
    fn discard(&self) -> wasmtime::Result<()>;

    /// What the display the target is on says of its range, as `adapter` of `instance` reads it.
    fn hdr_info(&self, instance: &Global, adapter: AdapterId) -> wgpu_types::DisplayHdrInfo;
}

/// A frame a target hands out: its texture where it has one, and how the target stands.
pub struct SurfaceFrame {
    pub status: wgpu_types::SurfaceStatus,
    pub texture: Option<Arc<GpuObject<TextureId>>>,
}

/// A surface as the program holds it, every handle on one target sharing its configuration.
#[derive(Clone)]
pub struct Surface(Arc<SurfaceState>);

/// A surface as an embedder keeps it to hand out again: weakly, keeping nothing alive.
pub struct WeakSurface(Weak<SurfaceState>);

impl WeakSurface {
    /// The surface, while a handle on it is still held.
    pub fn upgrade(&self) -> Option<Surface> {
        self.0.upgrade().map(Surface)
    }
}

struct SurfaceState {
    target: Arc<dyn SurfaceTarget>,
    configured: Mutex<Option<Configured>>,
}

/// What `configure` last set, and on which device.
#[derive(Clone)]
struct Configured {
    device: Device,
    configuration: wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
}

impl Surface {
    /// A surface over `target`, not configured yet.
    pub fn new(target: Arc<dyn SurfaceTarget>) -> Surface {
        Surface(Arc::new(SurfaceState {
            target,
            configured: Mutex::new(None),
        }))
    }

    /// The surface as an embedder keeps it.
    pub fn downgrade(&self) -> WeakSurface {
        WeakSurface(Arc::downgrade(&self.0))
    }

    pub(crate) fn target(&self) -> Arc<dyn SurfaceTarget> {
        Arc::clone(&self.0.target)
    }

    fn configured(&self) -> Option<Configured> {
        self.0.configured.lock().unwrap().clone()
    }

    /// The texture of `frame` as the program holds it: on the configured device, described by
    /// the configuration.
    fn frame_texture(configured: &Configured, texture: Arc<GpuObject<TextureId>>) -> Texture {
        let configuration = &configured.configuration;
        configured.device.texture(
            texture,
            wgpu_types::TextureDescriptor {
                label: None,
                size: wgpu_types::Extent3d {
                    width: configuration.width,
                    height: configuration.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu_types::TextureDimension::D2,
                format: configuration.format,
                usage: configuration.usage,
                view_formats: configuration.view_formats.clone(),
            },
        )
    }
}

impl gpu::HostSurface for GpuCtx<'_> {
    fn capabilities(
        &mut self,
        surface: Resource<Surface>,
        adapter: Resource<Adapter>,
    ) -> wasmtime::Result<SurfaceCapabilities> {
        let adapter = self.table.get(&adapter)?.id();
        let target = self.table.get(&surface)?.target();
        Ok(target.capabilities(self.instance, adapter).into())
    }

    fn configure(
        &mut self,
        surface: Resource<Surface>,
        device: Resource<Device>,
        configuration: SurfaceConfiguration,
    ) -> wasmtime::Result<()> {
        let device = self.table.get(&device)?.clone();
        let configuration = configuration.into();
        let surface = self.table.get(&surface)?;
        match surface.0.target.configure(&device, &configuration)? {
            Ok(()) => {
                *surface.0.configured.lock().unwrap() = Some(Configured {
                    device,
                    configuration,
                });
            }
            Err(error) => device
                .errors
                .report(Some(error), "Surface::configure", None),
        }
        Ok(())
    }

    fn configuration(
        &mut self,
        surface: Resource<Surface>,
    ) -> wasmtime::Result<Option<SurfaceConfiguration>> {
        let configured = self.table.get(&surface)?.configured();
        Ok(configured.map(|configured| (&configured.configuration).into()))
    }

    fn current_texture(
        &mut self,
        surface: Resource<Surface>,
    ) -> wasmtime::Result<CurrentSurfaceTexture> {
        let surface = self.table.get(&surface)?;
        let Some(configured) = surface.configured() else {
            wasmtime::bail!("Surface::get_current_texture before the surface is configured")
        };
        let frame = match surface.0.target.current_texture()? {
            Ok(frame) => frame,
            Err(refusal) => {
                (configured.device.errors).report(
                    Some(refusal),
                    "Surface::get_current_texture",
                    None,
                );
                return Ok(CurrentSurfaceTexture::Validation);
            }
        };
        let mut texture = || -> wasmtime::Result<Resource<Texture>> {
            let Some(texture) = frame.texture.clone() else {
                wasmtime::bail!(
                    "the surface's target answered {:?} with no texture",
                    frame.status
                )
            };
            Ok(self
                .table
                .push(Surface::frame_texture(&configured, texture))?)
        };
        Ok(match frame.status {
            wgpu_types::SurfaceStatus::Good => CurrentSurfaceTexture::Success(texture()?),
            wgpu_types::SurfaceStatus::Suboptimal => CurrentSurfaceTexture::Suboptimal(texture()?),
            wgpu_types::SurfaceStatus::Timeout => CurrentSurfaceTexture::Timeout,
            wgpu_types::SurfaceStatus::Occluded => CurrentSurfaceTexture::Occluded,
            wgpu_types::SurfaceStatus::Outdated => CurrentSurfaceTexture::Outdated,
            wgpu_types::SurfaceStatus::Lost => CurrentSurfaceTexture::Lost,
            wgpu_types::SurfaceStatus::Validation => CurrentSurfaceTexture::Validation,
        })
    }

    fn discard(&mut self, surface: Resource<Surface>) -> wasmtime::Result<()> {
        self.table.get(&surface)?.0.target.discard()
    }

    fn hdr_info(
        &mut self,
        surface: Resource<Surface>,
        adapter: Resource<Adapter>,
    ) -> wasmtime::Result<DisplayHdrInfo> {
        let adapter = self.table.get(&adapter)?.id();
        let target = self.table.get(&surface)?.target();
        Ok(target.hdr_info(self.instance, adapter).into())
    }

    fn drop(&mut self, surface: Resource<Surface>) -> wasmtime::Result<()> {
        self.table.delete(surface)?;
        Ok(())
    }
}
