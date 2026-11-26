//! A view's hardware buffers as everyone who reaches them does: the canvas on the program's thread,
//! the tick of the engine's time when the view has drawn nothing for a while, the shell when it
//! releases a hardware buffer, and the program's freeze. What decides which hardware buffers go is
//! here: the spares beyond the starting three once the view has drawn nothing for [`SPARE_IDLE`] of
//! the engine's time, and, while the program is frozen, every hardware buffer neither on screen nor
//! being drawn.
//!
//! The invariant: the allocation's lock is held for a read or a write of it, and nothing that
//! takes another lock is called while it is held, since the shell releases a hardware buffer from
//! wherever it is done with it.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use waos_gpu_host::{Device, GpuObject};

use super::pool::{HardwareBufferPool, Released};
use super::{SPARE_IDLE, STARTING_HARDWARE_BUFFERS};
use crate::BufferKind;
use crate::bindings::wasdk::platform::hardware_buffers::HardwareBufferUsage;
use crate::gpu::{wgpu_core, wgpu_types};
use crate::lifecycle::Lifecycle;
use crate::os::HardwareBufferMemory;
use crate::stats::Stats;
use crate::sync::locked;
use crate::targets;
use crate::views::ProgramView;

/// The bytes a pixel of a hardware buffer takes, in the one format a hardware buffer is allocated
/// in.
const BYTES_PER_PIXEL: u64 = 4;

/// A frame's texture, as the GPU host hands it to the program: the texture over the hardware
/// buffer the frame is drawn into, which goes back to wgpu-core once the program and the canvas
/// let go of it.
pub(super) type FrameTexture = Arc<GpuObject<wgpu_core::id::TextureId>>;

/// What a view's hardware buffers are allocated for: the program draws into them, the compositor
/// shows them, and a screenshot samples them.
fn view_usage() -> HardwareBufferUsage {
    HardwareBufferUsage::GPU_COLOR_OUTPUT
        | HardwareBufferUsage::COMPOSER_OVERLAY
        | HardwareBufferUsage::GPU_SAMPLED
}

/// A view's hardware buffers, once allocated, with the view they are drawn for, where their bytes
/// are counted, and the freeze they follow.
pub(super) struct ViewHardwareBuffers {
    allocation: Mutex<Option<Allocation>>,
    pub(super) view: ProgramView,
    pub(super) stats: Arc<Stats>,
    pub(super) lifecycle: Lifecycle,
    pub(super) hardware_buffers: BufferKind,
    quiet: Mutex<Quiet>,
}

/// Since when the view has drawn nothing, and whether its spares went since then: they go once
/// a time, until the next frame.
struct Quiet {
    since: Instant,
    spares_gone: bool,
}

/// The hardware buffers as `configure` allocated them, their bytes counted in the program's stats
/// for as long as they are held.
pub(super) struct Allocation {
    /// The program's device: what each frame's texture is made on, and whose queue says when a
    /// frame has finished drawing. The hardware buffers' own handle on it keeps it while a frame is
    /// still on the GPU after the program has let go of its device.
    pub(super) device: Device,
    /// What the program configured each frame's texture to be used for, and the formats besides
    /// the hardware buffers' own it is viewed in.
    pub(super) usage: wgpu_types::TextureUsages,
    pub(super) view_formats: Vec<wgpu_types::TextureFormat>,
    /// Every hardware buffer's size, in physical pixels.
    width: u32,
    height: u32,
    hardware_buffers: BufferKind,
    pub(super) pool: HardwareBufferPool<HardwareBufferMemory>,
    /// The texture of the frame being drawn, kept until it is presented: what the frame's
    /// hand-over to the compositor works on.
    drawing: Option<FrameTexture>,
    stats: Arc<Stats>,
}

/// A frame just presented: its hardware buffer, the shell's until it calls `released`, the
/// texture it was drawn through, for what the program configured it, and the device it was drawn
/// on.
pub(super) struct DrawnFrame {
    pub(super) memory: HardwareBufferMemory,
    pub(super) released: Released,
    pub(super) texture: FrameTexture,
    pub(super) usage: wgpu_types::TextureUsages,
    pub(super) device: Device,
}

impl ViewHardwareBuffers {
    /// No hardware buffers yet, for `view`, with their bytes counted in `stats`, following the
    /// freeze of `lifecycle`, quiet since `now`.
    pub(super) fn new(
        view: ProgramView,
        stats: Arc<Stats>,
        lifecycle: Lifecycle,
        hardware_buffers: BufferKind,
        now: Instant,
    ) -> Arc<ViewHardwareBuffers> {
        Arc::new(ViewHardwareBuffers {
            allocation: Mutex::new(None),
            view,
            stats,
            lifecycle,
            hardware_buffers,
            quiet: Mutex::new(Quiet {
                since: now,
                spares_gone: false,
            }),
        })
    }

    pub(super) fn allocation(&self) -> MutexGuard<'_, Option<Allocation>> {
        locked(&self.allocation)
    }

    /// The view presented a frame at `at`, on the engine's clock.
    pub(super) fn presented(&self, at: Instant) {
        *locked(&self.quiet) = Quiet {
            since: at,
            spares_gone: false,
        };
    }

    /// The engine's clock reads `now`: what the view has no use for goes. While its program is
    /// frozen that is every hardware buffer but the one on screen, which the view keeps, and the
    /// one being drawn, which the program finishes once it is unfrozen. Otherwise, once the view
    /// has drawn nothing for [`SPARE_IDLE`], the ones beyond the starting ones go, once until the
    /// next frame.
    pub(super) fn let_unused_go(&self, now: Instant) {
        if self.lifecycle.is_frozen() {
            return self.let_go_of_unused(0, "the program is frozen");
        }
        {
            let mut quiet = locked(&self.quiet);
            if quiet.spares_gone || now.saturating_duration_since(quiet.since) < SPARE_IDLE {
                return;
            }
            quiet.spares_gone = true;
        }
        self.let_go_of_unused(
            STARTING_HARDWARE_BUFFERS,
            "the view drew nothing for a while",
        );
    }

    /// The shell released a hardware buffer, which goes too while the program is frozen.
    pub(super) fn hardware_buffer_freed(&self) {
        if self.lifecycle.is_frozen() {
            self.let_go_of_unused(0, "the program is frozen");
        }
    }

    /// Lets go of the hardware buffers nobody holds or draws into, down to `keep`, saying `why` in
    /// the view's trace.
    fn let_go_of_unused(&self, keep: usize, why: &str) {
        let mut guard = self.allocation();
        let Some(allocation) = guard.as_mut() else {
            return;
        };
        let let_go = allocation.let_go_of_unused(keep);
        if let_go > 0 {
            tracing::trace!(
                target: targets::FRAMES,
                view = self.view.id(),
                "{why}: the pool lets {let_go} hardware buffer(s) go, {} now",
                allocation.pool.count()
            );
        }
    }
}

impl Allocation {
    /// The starting hardware buffers at `configuration`'s size, drawn on `device`, their bytes
    /// counted in `stats`.
    pub(super) fn new(
        configuration: &wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
        device: &Device,
        stats: &Arc<Stats>,
        hardware_buffers: BufferKind,
    ) -> anyhow::Result<Allocation> {
        let (width, height) = (configuration.width, configuration.height);
        let memories = (0..STARTING_HARDWARE_BUFFERS)
            .map(|_| HardwareBufferMemory::new(width, height, view_usage(), hardware_buffers))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let allocation = Allocation {
            device: device.clone(),
            usage: configuration.usage,
            view_formats: configuration.view_formats.clone(),
            width,
            height,
            hardware_buffers,
            pool: HardwareBufferPool::new(memories),
            drawing: None,
            stats: Arc::clone(stats),
        };
        stats.hardware_buffers_changed(0, allocation.bytes());
        Ok(allocation)
    }

    /// One hardware buffer more, at the size of the others; its index.
    pub(super) fn add_hardware_buffer(&mut self) -> anyhow::Result<usize> {
        let memory = HardwareBufferMemory::new(
            self.width,
            self.height,
            view_usage(),
            self.hardware_buffers,
        )?;
        let before = self.bytes();
        let index = self.pool.add(memory);
        self.stats.hardware_buffers_changed(before, self.bytes());
        Ok(index)
    }

    /// Hands hardware buffer `index` out for the frame being drawn through `texture`.
    pub(super) fn hand_out(&mut self, index: usize, texture: FrameTexture) {
        self.pool.hand_out(index);
        self.drawing = Some(texture);
    }

    /// Whether a frame is handed out and neither presented nor discarded.
    pub(super) fn is_drawing(&self) -> bool {
        self.drawing.is_some()
    }

    /// The frame being drawn, given back unpresented: its hardware buffer is free again.
    pub(super) fn discard(&mut self) {
        self.drawing = None;
        self.pool.discard_drawn();
    }

    /// The frame being drawn, now presented, its hardware buffer marked as the shell's with `freed`
    /// run once the shell releases it; `None` when nothing was handed out since the last present.
    pub(super) fn present_drawn(
        &mut self,
        freed: impl Fn() + Send + Sync + 'static,
    ) -> Option<DrawnFrame> {
        let texture = self.drawing.take()?;
        let (memory, released) = self.pool.present_drawn(freed)?;
        Some(DrawnFrame {
            memory: memory.clone(),
            released,
            texture,
            usage: self.usage,
            device: self.device.clone(),
        })
    }

    /// Lets go of the hardware buffers nobody holds or draws into, down to `keep`; answers how many
    /// went. A frame the shell still shows keeps its own hardware buffer alive through the handle
    /// it holds.
    fn let_go_of_unused(&mut self, keep: usize) -> usize {
        let before = self.bytes();
        let let_go = self.pool.let_go_of_unused(keep);
        self.stats.hardware_buffers_changed(before, self.bytes());
        let_go
    }

    fn bytes(&self) -> u64 {
        let buffer = u64::from(self.width) * u64::from(self.height) * BYTES_PER_PIXEL;
        self.pool.count() as u64 * buffer
    }
}

impl Drop for Allocation {
    /// The hardware buffers go with the allocation, replaced at a new size or with the canvas, and
    /// so do their bytes.
    fn drop(&mut self) {
        self.stats.hardware_buffers_changed(self.bytes(), 0);
    }
}
