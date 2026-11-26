//! A view's canvas: the target of the view's surface (`wasdk:gpu`'s), over the view's hardware
//! buffers, and how a drawn frame is presented. The canvas is the one owner of the view's hardware
//! buffers and of their handle on the program's device: it lives as long as the program's handles
//! on the view's surface, and the shell keeps a presented hardware buffer on its own.
//!
//! The canvas is the size the program configured its surface at, never the view's: its hardware
//! buffers are allocated at that size when the surface is configured, and again when the program
//! configures another; a change of the view's size changes nothing here.
//!
//! Every handle a program takes on one view's surface is the same surface over the same canvas:
//! a program's surfaces are kept by view, weakly, so a canvas goes when the program drops its last
//! handle on it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use waos_gpu_host::{Device, GpuObject, Surface, SurfaceFrame, SurfaceTarget, WeakSurface};
use wasmtime::bail;
use wgpu_core::present::ConfigureSurfaceError;

use super::finisher::{FrameFinisher, Wait};
use super::view_hardware_buffers::{Allocation, DrawnFrame, FrameTexture, ViewHardwareBuffers};
use crate::BufferKind;
use crate::bindings::wasdk::platform::views::ViewId;
use crate::engine_time::{EngineClock, Timer};
use crate::gpu::{
    GpuInstance, InFlight, submit_once_running, submit_while_running, wgpu_core, wgpu_types,
};
use crate::lifecycle::Lifecycle;
use crate::os::{self, FORMAT};
use crate::stats::Stats;
use crate::sync::locked;
use crate::targets;
use crate::views::{PlacedShellLayer, ProgramView};

/// What a program may configure its canvas to use its textures for: drawing into them, sampling
/// them, and copying to and from them. What each system makes over a hardware buffer serves each of
/// these; writing from a shader would need more of it.
const USAGES: wgpu_types::TextureUsages = wgpu_types::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu_types::TextureUsages::TEXTURE_BINDING)
    .union(wgpu_types::TextureUsages::COPY_SRC)
    .union(wgpu_types::TextureUsages::COPY_DST);

const NOT_CONFIGURED: &str = "the view's surface is not configured";

/// How the frames wait for the screen: in order, one each time the view is shown; the automatic
/// modes come to this.
const PRESENT_MODES: [wgpu_types::PresentMode; 3] = [
    wgpu_types::PresentMode::Fifo,
    wgpu_types::PresentMode::AutoVsync,
    wgpu_types::PresentMode::AutoNoVsync,
];

/// How the shell composes a frame's alpha: ignored, or premultiplied; the automatic mode is the
/// first.
const ALPHA_MODES: [wgpu_types::CompositeAlphaMode; 3] = [
    wgpu_types::CompositeAlphaMode::Opaque,
    wgpu_types::CompositeAlphaMode::PreMultiplied,
    wgpu_types::CompositeAlphaMode::Auto,
];

/// A view's hardware buffers, and the thread that hands each drawn frame to the shell.
pub(crate) struct ViewCanvas {
    gpu: GpuInstance,
    buffers: Arc<ViewHardwareBuffers>,
    finisher: FrameFinisher,
    /// The program's GPU work in flight, which every wait of the thread counts in.
    in_flight: Arc<InFlight>,
    /// The engine's clock, which a present is timed by.
    clock: EngineClock,
    /// What lets the spares go at a tick of the engine's time once the view has drawn nothing
    /// for a while; the clock holds it only while the canvas does.
    _spares_timer: Arc<Timer>,
}

impl ViewCanvas {
    /// A canvas for `view`, drawn on `gpu`, its hardware buffers' bytes and its frames counted in
    /// `stats`, its hardware buffers following the freeze of `lifecycle` and the time of `clock`,
    /// and its waits counted in `in_flight`.
    pub(crate) fn new(
        view: ProgramView,
        gpu: GpuInstance,
        stats: Arc<Stats>,
        lifecycle: Lifecycle,
        clock: EngineClock,
        in_flight: Arc<InFlight>,
        hardware_buffers: BufferKind,
    ) -> Arc<ViewCanvas> {
        let id = view.id();
        let buffers =
            ViewHardwareBuffers::new(view, stats, lifecycle, hardware_buffers, clock.now());
        let quiet = Arc::downgrade(&buffers);
        let spares_timer: Arc<Timer> = Arc::new(move |now| {
            if let Some(buffers) = quiet.upgrade() {
                buffers.let_unused_go(now);
            }
        });
        clock.at_each_tick(&spares_timer);
        Arc::new(ViewCanvas {
            gpu: Arc::clone(&gpu),
            buffers,
            finisher: FrameFinisher::start(gpu, id),
            in_flight,
            clock,
            _spares_timer: spares_timer,
        })
    }

    /// Hands the frame just drawn to the shell once the GPU has finished it, with `layers`, the
    /// view's, beneath it, without waiting for the GPU here: what the program wrote on its
    /// device's queue and no submission carried is submitted first, once the program may submit,
    /// with what the system needs to hand the frame's memory to its compositor
    /// (`os::hand_to_compositor`), and the view's thread waits for that submission, after every
    /// one that drew the frame, and hands the frame over. Answers whether there was one, which a frame skipped on a lost
    /// device is not, nor a present with nothing drawn since the last; the layers of a present
    /// that hands nothing over are not shown, and the shell keeps those of the frame on screen.
    pub(crate) async fn present(&self, layers: Vec<PlacedShellLayer>) -> wasmtime::Result<bool> {
        let Some(DrawnFrame {
            memory,
            released,
            texture,
            usage,
            device,
        }) = self.drawn_frame()
        else {
            return Ok(false);
        };
        let hand_over_work = submit_once_running(&self.buffers.lifecycle, || {
            os::hand_to_compositor(&self.gpu, &device, &memory, texture.id(), usage)
        })
        .await;
        drop(texture);
        self.buffers.presented(self.clock.now());
        let view = self.buffers.view.clone();
        let frame = view.frame_in_flight();
        // Read on the program's thread as it presents: the events the picture was drawn after.
        let events_read = view.events_read();
        let stats = Arc::clone(&self.buffers.stats);
        let hand_over = move || {
            view.present(memory.buffer(), layers, released, events_read);
            stats.frame_presented(view.clock().now());
        };
        self.finisher.finish(Wait {
            submission: hand_over_work.submission,
            finish: hand_over_work.finish,
            device,
            hand_over: Box::new(hand_over),
            in_flight: self.in_flight.begin(),
            frame,
        });
        Ok(true)
    }

    /// The frame to hand over, its hardware buffer marked as the shell's until the shell releases
    /// it; `None` when nothing was drawn since the last present, or the canvas was never
    /// configured.
    fn drawn_frame(&self) -> Option<DrawnFrame> {
        let mut guard = self.buffers.allocation();
        let allocation = guard.as_mut()?;
        let buffers = Arc::downgrade(&self.buffers);
        allocation.present_drawn(move || {
            if let Some(buffers) = buffers.upgrade() {
                buffers.hardware_buffer_freed();
            }
        })
    }

    /// A free hardware buffer, or one added when the shell holds every one: a frame never waits for
    /// the shell to release one.
    fn hardware_buffer_for_next_frame(
        &self,
        allocation: &mut Allocation,
    ) -> wasmtime::Result<usize> {
        if let Some(index) = allocation.pool.first_free() {
            return Ok(index);
        }
        let index = allocation
            .add_hardware_buffer()
            .map_err(wasmtime::Error::from_anyhow)?;
        tracing::trace!(
            target: targets::FRAMES,
            view = self.buffers.view.id(),
            "every hardware buffer is held: the pool adds one, {} now",
            allocation.pool.count()
        );
        Ok(index)
    }
}

impl SurfaceTarget for ViewCanvas {
    /// The hardware buffers' one format, in sRGB, on any adapter: drawn into, sampled and copied
    /// from, presented in order, opaque or with its alpha premultiplied.
    fn capabilities(
        &self,
        _instance: &wgpu_core::global::Global,
        _adapter: wgpu_core::id::AdapterId,
    ) -> wgpu_types::SurfaceCapabilities {
        wgpu_types::SurfaceCapabilities {
            formats: vec![FORMAT],
            format_capabilities: vec![wgpu_types::SurfaceFormatCapabilities {
                format: FORMAT,
                color_spaces: wgpu_types::SurfaceColorSpaces::SRGB,
            }],
            present_modes: PRESENT_MODES[..1].to_vec(),
            alpha_modes: ALPHA_MODES[..2].to_vec(),
            usages: USAGES,
        }
    }

    /// Allocates the hardware buffers at the size `configuration` gives, to be drawn on `device`
    /// and finished on its queue, each frame's texture made for the usage it asks and viewable in
    /// the formats it names. What the hardware buffers cannot serve is refused rather than left
    /// out, since a frame that relied on it would be rejected by the GPU, as is a size of zero or
    /// past the device's largest 2D texture. An allocation the system refuses traps the program.
    fn configure(
        &self,
        device: &Device,
        configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
    ) -> wasmtime::Result<Result<(), ConfigureSurfaceError>> {
        let largest = self
            .gpu
            .device_limits(device.device_id())
            .max_texture_dimension_2d;
        if let Err(refusal) = check_size(configuration, largest).and(check_served(configuration)) {
            return Ok(Err(refusal));
        }
        let allocation = Allocation::new(
            configuration,
            device,
            &self.buffers.stats,
            self.buffers.hardware_buffers,
        )
        .map_err(wasmtime::Error::from_anyhow)?;
        // The hardware buffers that were here belong to the old size. A frame the shell is still
        // showing keeps its own alive through the hardware buffer it holds.
        *self.buffers.allocation() = Some(allocation);
        Ok(Ok(()))
    }

    /// The texture for the frame being drawn, over a free hardware buffer, or one added when the
    /// shell holds every hardware buffer, taken back from the compositor for the program's device
    /// where the system needs it (`os::take_from_compositor`), once the program may submit. A
    /// program that takes it while frozen draws into it without that: the texture arrives with its
    /// pixels undefined, which a frame drawn whole never reads, and the system then needs nothing
    /// taken back.
    ///
    /// On a lost device the texture is invalid, as wgpu-core's surfaces answer there,
    /// and the hardware buffer is not handed out: the frame draws nothing and is skipped.
    fn current_texture(&self) -> wasmtime::Result<SurfaceFrame> {
        let mut guard = self.buffers.allocation();
        let Some(allocation) = guard.as_mut() else {
            bail!(NOT_CONFIGURED)
        };
        let index = self.hardware_buffer_for_next_frame(allocation)?;
        let texture = allocation
            .pool
            .memory(index)
            .texture_over(
                &self.gpu,
                allocation.device.device_id(),
                allocation.usage,
                &allocation.view_formats,
            )
            .map_err(wasmtime::Error::from_anyhow)?;
        let texture: FrameTexture = Arc::new(GpuObject::new(texture, &self.gpu));
        if allocation.device.is_lost() {
            return Ok(drawn_into(texture));
        }
        allocation.hand_out(index, Arc::clone(&texture));
        let (device, usage) = (allocation.device.clone(), allocation.usage);
        let memory = allocation.pool.memory(index).clone();
        drop(guard);
        let taken = submit_while_running(&self.buffers.lifecycle, &mut || {
            os::take_from_compositor(&self.gpu, &device, &memory, texture.id(), usage);
        });
        if !taken {
            tracing::debug!(
                target: targets::FRAMES,
                view = self.buffers.view.id(),
                "a frame's texture is taken while the program is frozen: drawn into as it arrives"
            );
        }
        Ok(drawn_into(texture))
    }

    /// What sRGB says of the range: the system's display is not read yet.
    fn hdr_info(
        &self,
        _instance: &wgpu_core::global::Global,
        _adapter: wgpu_core::id::AdapterId,
    ) -> wgpu_types::DisplayHdrInfo {
        wgpu_types::DisplayHdrInfo::default()
    }
}

/// A frame drawn into `texture`, which the view shows when presented.
fn drawn_into(texture: FrameTexture) -> SurfaceFrame {
    SurfaceFrame {
        status: wgpu_types::SurfaceStatus::Good,
        texture: Some(texture),
    }
}

/// Refuses a size of zero, or past `largest`, the device's largest 2D texture on a side.
fn check_size(
    configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
    largest: u32,
) -> Result<(), ConfigureSurfaceError> {
    let (width, height) = (configuration.width, configuration.height);
    if width == 0 || height == 0 {
        return Err(ConfigureSurfaceError::ZeroArea);
    }
    if width > largest || height > largest {
        return Err(ConfigureSurfaceError::TooLarge {
            width,
            height,
            max_texture_dimension_2d: largest,
        });
    }
    Ok(())
}

/// Refuses a configuration asking for a format, a view format, a usage, a present mode, an
/// alpha mode or a colour space the hardware buffers cannot serve. A texture over one is viewed
/// in its own format or its sRGB counterpart, as wgpu's surfaces allow.
fn check_served(
    configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
) -> Result<(), ConfigureSurfaceError> {
    let format = configuration.format;
    if format != FORMAT {
        return Err(ConfigureSurfaceError::UnsupportedFormat {
            requested: format,
            available: vec![FORMAT],
        });
    }
    if let Some(&view_format) = configuration
        .view_formats
        .iter()
        .find(|view_format| view_format.remove_srgb_suffix() != FORMAT)
    {
        return Err(ConfigureSurfaceError::InvalidViewFormat(
            view_format,
            FORMAT,
        ));
    }
    if !USAGES.contains(configuration.usage) {
        return Err(ConfigureSurfaceError::UnsupportedUsage {
            requested: texture_uses(configuration.usage),
            available: texture_uses(USAGES),
        });
    }
    if !PRESENT_MODES.contains(&configuration.present_mode) {
        return Err(ConfigureSurfaceError::UnsupportedPresentMode {
            requested: configuration.present_mode,
            available: PRESENT_MODES[..1].to_vec(),
        });
    }
    if !ALPHA_MODES.contains(&configuration.alpha_mode) {
        return Err(ConfigureSurfaceError::UnsupportedAlphaMode {
            requested: configuration.alpha_mode,
            available: ALPHA_MODES[..2].to_vec(),
        });
    }
    let color_space = configuration.color_space;
    if !matches!(
        color_space,
        wgpu_types::SurfaceColorSpace::Auto | wgpu_types::SurfaceColorSpace::Srgb
    ) {
        return Err(ConfigureSurfaceError::UnsupportedColorSpace {
            requested: color_space,
            format,
            available: wgpu_types::SurfaceColorSpaces::SRGB,
        });
    }
    Ok(())
}

/// `usage` as the uses wgpu-core names a surface's refusal in.
fn texture_uses(usage: wgpu_types::TextureUsages) -> wgpu_types::TextureUses {
    use wgpu_types::{TextureUsages as Usage, TextureUses as Uses};
    [
        (Usage::COPY_SRC, Uses::COPY_SRC),
        (Usage::COPY_DST, Uses::COPY_DST),
        (Usage::TEXTURE_BINDING, Uses::RESOURCE),
        (Usage::STORAGE_BINDING, Uses::STORAGE_READ_WRITE),
        (Usage::RENDER_ATTACHMENT, Uses::COLOR_TARGET),
        (Usage::STORAGE_ATOMIC, Uses::STORAGE_ATOMIC),
        (Usage::TRANSIENT_ATTACHMENT, Uses::TRANSIENT),
    ]
    .into_iter()
    .filter(|(flag, _)| usage.contains(*flag))
    .fold(Uses::empty(), |uses, (_, used)| uses | used)
}

/// One program's views' surfaces, by view: each handle the program takes on a view's surface is
/// the one surface over the view's canvas, and a view gets a new one once the program has dropped
/// every handle on the last.
#[derive(Default)]
pub(crate) struct Canvases(Mutex<HashMap<ViewId, ViewSurface>>);

/// A view's surface and its canvas, kept weakly: the surface holds the canvas.
struct ViewSurface {
    surface: WeakSurface,
    canvas: Weak<ViewCanvas>,
}

impl Canvases {
    /// The surface of `view`, over the canvas `make` makes when it has none.
    pub(crate) fn surface_of(
        &self,
        view: ViewId,
        make: impl FnOnce() -> Arc<ViewCanvas>,
    ) -> Surface {
        let mut canvases = locked(&self.0);
        if let Some(surface) = canvases.get(&view).and_then(|kept| kept.surface.upgrade()) {
            return surface;
        }
        let canvas = make();
        let surface = Surface::new(Arc::clone(&canvas) as Arc<dyn SurfaceTarget>);
        canvases.insert(
            view,
            ViewSurface {
                surface: surface.downgrade(),
                canvas: Arc::downgrade(&canvas),
            },
        );
        surface
    }

    /// The canvas of `view`, while the program holds its surface.
    pub(crate) fn of_view(&self, view: ViewId) -> Option<Arc<ViewCanvas>> {
        locked(&self.0)
            .get(&view)
            .and_then(|kept| kept.canvas.upgrade())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas_configuration(
        format: wgpu_types::TextureFormat,
        usage: wgpu_types::TextureUsages,
    ) -> wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>> {
        wgpu_types::SurfaceConfiguration {
            usage,
            format,
            width: 4,
            height: 4,
            present_mode: wgpu_types::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu_types::CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
            color_space: wgpu_types::SurfaceColorSpace::default(),
        }
    }

    fn viewed_as(
        view_formats: Vec<wgpu_types::TextureFormat>,
    ) -> wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>> {
        wgpu_types::SurfaceConfiguration {
            view_formats,
            ..canvas_configuration(FORMAT, wgpu_types::TextureUsages::RENDER_ATTACHMENT)
        }
    }

    /// A renderer that draws in linear colour and lets the GPU encode sRGB, as Bevy does, views
    /// the canvas in its sRGB counterpart.
    #[test]
    fn a_canvas_may_be_viewed_in_its_own_format_or_its_srgb_counterpart() {
        let view_formats = vec![FORMAT, FORMAT.add_srgb_suffix()];
        assert!(check_served(&viewed_as(view_formats)).is_ok());
    }

    #[test]
    fn a_view_format_other_than_the_canvas_format_or_its_srgb_counterpart_is_refused() {
        let other = wgpu_types::TextureFormat::Rgba8UnormSrgb;
        let refusal = check_served(&viewed_as(vec![other])).unwrap_err();
        assert!(
            matches!(refusal, ConfigureSurfaceError::InvalidViewFormat(..)),
            "{refusal}"
        );
    }

    #[test]
    fn a_canvas_past_the_devices_largest_texture_is_refused() {
        let configuration =
            canvas_configuration(FORMAT, wgpu_types::TextureUsages::RENDER_ATTACHMENT);
        assert!(check_size(&configuration, 4).is_ok());
        let refusal = check_size(&configuration, 3).unwrap_err();
        assert!(
            matches!(refusal, ConfigureSurfaceError::TooLarge { width: 4, .. }),
            "{refusal}"
        );
    }

    /// A renderer that snapshots its target for a backdrop blur copies from the canvas.
    #[test]
    fn a_canvas_may_be_drawn_into_and_copied_from() {
        let usage =
            wgpu_types::TextureUsages::RENDER_ATTACHMENT | wgpu_types::TextureUsages::COPY_SRC;
        assert!(check_served(&canvas_configuration(FORMAT, usage)).is_ok());
    }

    #[test]
    fn a_usage_the_hardware_buffers_cannot_serve_is_refused() {
        let usage = wgpu_types::TextureUsages::RENDER_ATTACHMENT
            | wgpu_types::TextureUsages::STORAGE_BINDING;
        let refusal = check_served(&canvas_configuration(FORMAT, usage)).unwrap_err();
        assert!(
            matches!(refusal, ConfigureSurfaceError::UnsupportedUsage { .. }),
            "{refusal}"
        );
    }

    #[test]
    fn a_format_other_than_the_hardware_buffers_is_refused() {
        let format = wgpu_types::TextureFormat::Rgba16Float;
        let usage = wgpu_types::TextureUsages::RENDER_ATTACHMENT;
        assert!(check_served(&canvas_configuration(format, usage)).is_err());
    }
}
