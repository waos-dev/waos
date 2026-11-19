//! A hardware buffer a program holds: one it allocated, an image the hardware codecs decoded for
//! it, both counted in the program's stats (`counted_bytes`), or a video frame's, which keeps the
//! frame and its slot in the decoder's pool (decision 35 of
//! `docs/proposals/2026-11-02-program-media.md`). Its usage is what its memory answers it can be
//! used for (`os`), at least what the program asked for one it allocated, which it keeps too, so
//! that a read beyond the ask is told apart (`unasked_read`); `gpu_interop` works out from the usage
//! what a texture of it is for, and its pixels are read only where the usage has `cpu-read`. Its
//! textures are made on the
//! program's devices over its memory, and each holds the buffer's memory until wgpu frees it, once
//! the GPU is done with it. Where the system hands a texture's memory over (`os::HANDS_TEXTURES_OVER`),
//! the buffer holds its textures in turn until it is next handed out (below), since a program may
//! draw through a texture it lets go of at once; elsewhere it keeps only those the program holds.
//!
//! The program draws into a buffer with work it submits and writes it makes on its queue, which
//! nothing tells the system of: so whatever reads the buffer outside the program's GPU work, its
//! pixels read back, the codecs encoding it or the compositor showing it, first submits what the
//! program wrote on each device a texture of the buffer was made on and no submission carried, then
//! waits for everything the program submitted there ([`SubmittedWork`]). Where the system needs it,
//! the buffer is handed to them with that submission and taken back once the last of them is done,
//! before the program hears it is back (`hand_out`, `take_back`): on Android each live texture's
//! image is released to the compositor's queue family, and acquired back with its pixels kept.
//!
//! The invariants: a buffer's memory, its frame and its counted bytes go together (`Pixels`), once
//! the program and every texture, read, encoding and present holding the buffer have let go of
//! it, a texture holding the pixels and never the buffer, which holds the texture; and a buffer is
//! handed out once however many readers hold it, and taken back once the last is done, both under
//! the lock that counts them.

use std::sync::{Arc, Mutex};

use waos_webgpu_host::{Device, GpuObject, Texture, WeakDevice};

use super::Released;
use super::counted_bytes::CountedBytes;
use crate::HardwareBufferMode;
use crate::bindings::wasdk::platform::hardware_buffers::{
    HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use crate::gpu::{GpuInstance, submit_once_thawed, wait_for, wgpu_core, wgpu_types};
use crate::lifecycle::Lifecycle;
use crate::os::{self, FrameMemory, HardwareBufferMemory, LARGEST_SIDE};
use crate::shell::HardwareBuffer;
use crate::stats::Stats;
use crate::sync::locked;

/// `hardware-buffers.hardware-buffer`: a hardware buffer the program holds. Cloning it is another
/// hold on the same buffer.
#[derive(Clone)]
pub struct ProgramHardwareBuffer(Arc<HeldBuffer>);

/// What one buffer holds.
struct HeldBuffer {
    pixels: Arc<Pixels>,
    /// What the program asked for at allocation; none for a buffer it did not allocate.
    asked: Option<HardwareBufferUsage>,
    /// The devices its textures were made on, whose work drew into it.
    devices: Mutex<Vec<WeakDevice>>,
    /// Its textures while they live, which are handed out and taken back with it.
    textures: Mutex<Vec<MadeTexture>>,
    /// How many readers outside the program's GPU work hold it now: a present, a read of its
    /// pixels, an encoding.
    readers: Mutex<usize>,
}

/// A buffer's memory, as its textures hold it: the memory, with the frame it is the memory of and
/// the bytes the program is counted for.
struct Pixels {
    memory: HardwareBufferMemory,
    /// The frame whose memory it is, kept with its slot in the decoder's pool; none for a buffer
    /// of the program's own.
    _frame: Option<FrameMemory>,
    /// The bytes the program is counted for holding it; none for a frame's.
    _counted: Option<CountedBytes>,
}

/// A texture of a buffer, held, with its device and what it is for.
type LiveTexture = (
    Arc<GpuObject<wgpu_core::id::TextureId>>,
    Device,
    wgpu_types::TextureUsages,
);

/// A texture made of a buffer, as the buffer keeps it until it is next handed out, or past that
/// while the program holds it: with its device and what it is for.
struct MadeTexture {
    texture: Arc<GpuObject<wgpu_core::id::TextureId>>,
    device: Device,
    usage: wgpu_types::TextureUsages,
}

impl ProgramHardwareBuffer {
    /// A buffer the program allocated, of `width` by `height` in `format`, for `usage`, counted in
    /// `stats`: `too-large` for a side past the largest this system allocates, and the system's
    /// refusals. Sides of no pixels are the caller's to refuse.
    pub(crate) fn allocate(
        stats: Arc<Stats>,
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
        mode: HardwareBufferMode,
    ) -> Result<ProgramHardwareBuffer, ImageError> {
        if width.max(height) > LARGEST_SIDE {
            return Err(ImageError {
                kind: ImageErrorKind::TooLarge,
                message: format!(
                    "a side past {LARGEST_SIDE} pixels, the largest the system makes a texture of"
                ),
            });
        }
        let memory = HardwareBufferMemory::allocate(width, height, format, usage, mode)?;
        let counted = CountedBytes::new(stats, memory.bytes());
        Ok(ProgramHardwareBuffer::held(
            memory,
            Some(usage),
            None,
            Some(counted),
        ))
    }

    /// `decoded`, an image the hardware codecs decoded into a buffer of the program's own, for
    /// what the system made it for, counted in `stats`.
    pub(crate) fn decoded(stats: Arc<Stats>, decoded: HardwareBuffer) -> ProgramHardwareBuffer {
        let memory = HardwareBufferMemory::of(decoded);
        let counted = CountedBytes::new(stats, memory.bytes());
        ProgramHardwareBuffer::held(memory, None, None, Some(counted))
    }

    /// The buffer of `frame`, for what the system made it for, which keeps it. A failure is the
    /// shell's, which handed out a frame of a kind the engine does not take.
    pub(crate) fn of_frame(frame: &FrameMemory) -> anyhow::Result<ProgramHardwareBuffer> {
        let memory = frame.hardware_buffer()?;
        Ok(ProgramHardwareBuffer::held(
            memory,
            None,
            Some(frame.clone()),
            None,
        ))
    }

    fn held(
        memory: HardwareBufferMemory,
        asked: Option<HardwareBufferUsage>,
        frame: Option<FrameMemory>,
        counted: Option<CountedBytes>,
    ) -> ProgramHardwareBuffer {
        ProgramHardwareBuffer(Arc::new(HeldBuffer {
            pixels: Arc::new(Pixels {
                memory,
                _frame: frame,
                _counted: counted,
            }),
            asked,
            devices: Mutex::default(),
            textures: Mutex::default(),
            readers: Mutex::new(0),
        }))
    }

    pub(crate) fn width(&self) -> u32 {
        self.0.pixels.memory.width()
    }

    pub(crate) fn height(&self) -> u32 {
        self.0.pixels.memory.height()
    }

    /// What the buffer can be used for: at least what the program asked, for one it allocated.
    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        self.0.pixels.memory.usage()
    }

    /// Whether the CPU reads the buffer though the program did not ask for that when it allocated
    /// it, which works only because this system gives the buffer more; never for a buffer the
    /// program did not allocate.
    pub(crate) fn reads_unasked(&self) -> bool {
        let read = HardwareBufferUsage::CPU_READ;
        self.0
            .asked
            .is_some_and(|asked| self.usage().contains(read) && !asked.contains(read))
    }

    /// The buffer as the shell is handed it, to encode or to show: a retain of its memory, which
    /// the caller keeps this buffer held for as long as the shell has it.
    pub(crate) fn shell_buffer(&self) -> HardwareBuffer {
        self.0.pixels.memory.buffer()
    }

    /// `released`, the shell's release of the buffer, held back while a screenshot reads it: on a
    /// system whose compositor knows nothing of the engine's readers, the release runs once the
    /// last of them is done.
    pub(crate) fn after_reads(&self, released: Released) -> Released {
        self.0.pixels.memory.after_reads(released)
    }

    /// A texture over the buffer's memory on `device` of `gpu`, for `usages`, holding the buffer
    /// until wgpu frees it. A failure the system answers is the engine's.
    pub(crate) fn texture_on(
        &self,
        gpu: &GpuInstance,
        device: &Device,
        usages: wgpu_types::TextureUsages,
    ) -> anyhow::Result<Texture> {
        let pixels = Arc::clone(&self.0.pixels);
        let release = Box::new(move || drop(pixels));
        let (texture, attributes) = self
            .0
            .pixels
            .memory
            .program_texture(gpu, device, usages, release)?;
        {
            let mut textures = locked(&self.0.textures);
            if !os::HANDS_TEXTURES_OVER {
                textures.retain(|made| Arc::strong_count(&made.texture) > 1);
            }
            textures.push(MadeTexture {
                texture: Arc::clone(&texture),
                device: device.clone(),
                usage: usages,
            });
        }
        let mut devices = locked(&self.0.devices);
        devices.retain(|known| known.upgrade().is_some());
        if !devices.iter().any(|known| {
            known
                .upgrade()
                .is_some_and(|known| known.id() == device.device_id())
        }) {
            devices.push(device.downgrade());
        }
        Ok(device.connect_texture(texture, attributes))
    }

    /// The buffer handed to a reader outside the program's GPU work, once the program of
    /// `lifecycle` may submit: what the program wrote and no submission carried is submitted on
    /// the devices that drew into it, with, for the first reader, what the system needs to hand
    /// each live texture's memory over (`os::hand_to_compositor`); answers the work to wait for,
    /// nothing for a buffer no texture was made of. The reader gives it back with `take_back`.
    pub(crate) async fn hand_out(&self, gpu: &GpuInstance, lifecycle: &Lifecycle) -> SubmittedWork {
        submit_once_thawed(lifecycle, || {
            let mut readers = locked(&self.0.readers);
            *readers += 1;
            let mut work = if *readers == 1 {
                self.hand_textures_over(gpu)
            } else {
                Vec::new()
            };
            let devices = locked(&self.0.devices).clone();
            work.extend(
                devices
                    .iter()
                    .filter_map(WeakDevice::submit_pending_writes)
                    .map(|(device, submission)| (device, os::HandOver::submission(submission))),
            );
            SubmittedWork(work)
        })
        .await
    }

    /// A reader handed the buffer by `hand_out` is done with it: once the last is, each live
    /// texture is taken back for the program's device (`os::take_back_from_compositor`), once the
    /// program of `lifecycle` may submit, before the caller tells the program the buffer is back.
    pub(crate) async fn take_back(&self, gpu: &GpuInstance, lifecycle: &Lifecycle) {
        submit_once_thawed(lifecycle, || {
            let mut readers = locked(&self.0.readers);
            *readers = readers.saturating_sub(1);
            if *readers == 0 {
                for (texture, device, usage) in self.live_textures() {
                    os::take_back_from_compositor(
                        gpu,
                        &device,
                        &self.0.pixels.memory,
                        texture.id(),
                        usage,
                    );
                }
            }
        })
        .await;
    }

    /// Each texture handed over, with the submission that did it on its device; those the program
    /// no longer holds are let go of then, the submission keeping them for the GPU.
    fn hand_textures_over(
        &self,
        gpu: &GpuInstance,
    ) -> Vec<(Arc<GpuObject<wgpu_core::id::DeviceId>>, os::HandOver)> {
        let handed = self
            .live_textures()
            .into_iter()
            .filter_map(|(texture, device, usage)| {
                let hand_over = os::hand_to_compositor(
                    gpu,
                    &device,
                    &self.0.pixels.memory,
                    texture.id(),
                    usage,
                );
                Some((device.downgrade().upgrade()?, hand_over))
            })
            .collect();
        locked(&self.0.textures).retain(|made| Arc::strong_count(&made.texture) > 1);
        handed
    }

    /// The textures the buffer keeps, held while the answer is.
    fn live_textures(&self) -> Vec<LiveTexture> {
        locked(&self.0.textures)
            .iter()
            .map(|made| (Arc::clone(&made.texture), made.device.clone(), made.usage))
            .collect()
    }

    /// The buffer's pixels copied into the program's memory, as they lie now: `unsupported` for a
    /// buffer whose usage lacks `cpu-read`, on every system, since some of Android's allocators
    /// lock a buffer for the CPU whatever its usage and the emulator's refuses one without it
    /// (measured: the Pixel's allocator allowed the read, the emulator refused it).
    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        refusal_to_read(self.usage()).map_or_else(|| self.0.pixels.memory.read_pixels(), Err)
    }
}

/// Why the pixels of a buffer of `usage` are not read: `unsupported` where it lacks `cpu-read`.
fn refusal_to_read(usage: HardwareBufferUsage) -> Option<ImageError> {
    if usage.contains(HardwareBufferUsage::CPU_READ) {
        return None;
    }
    Some(ImageError {
        kind: ImageErrorKind::Unsupported,
        message: "a hardware buffer whose usage lacks `cpu-read`, which the CPU may not read"
            .to_owned(),
    })
}

/// What a program submitted and wrote on some of its devices, to wait for before the buffer it
/// drew into is read: each device, held for the wait, with the submission that carried what it
/// wrote, the last there; none where it was refused, which waits for the last submission.
pub(crate) struct SubmittedWork(Vec<(Arc<GpuObject<wgpu_core::id::DeviceId>>, os::HandOver)>);

impl SubmittedWork {
    /// Whether there is nothing to wait for: no texture of the buffer was ever made.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Waits, blocking, until the GPU has finished it and any post-wait pixel transfer has run.
    pub(crate) fn wait(self, gpu: &wgpu_core::global::Global) {
        for (device, hand_over) in self.0 {
            wait_for(gpu, device.id(), hand_over.submission);
            if let Some(finish) = hand_over.finish {
                finish(gpu);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_read_exactly_where_the_usage_has_cpu_read() {
        let refused = refusal_to_read(
            HardwareBufferUsage::GPU_SAMPLED | HardwareBufferUsage::GPU_COLOR_OUTPUT,
        );
        assert_eq!(
            refused.map(|error| error.kind),
            Some(ImageErrorKind::Unsupported)
        );
        assert!(refusal_to_read(HardwareBufferUsage::CPU_READ).is_none());
    }
}
