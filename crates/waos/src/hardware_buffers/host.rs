//! The code serving `wasdk:platform/hardware-buffers` to a program: the system's allocator, where
//! the engine allocates buffers, a buffer the program allocates with it, counted in its stats, and
//! any buffer it holds, its own or a video frame's: its size, what it can be used for, and its
//! pixels copied into the program's memory once the GPU has finished what the program submitted
//! and wrote to draw into it (`os`), where its usage has `cpu-read`, the program warned where that
//! is more than it asked (`unasked_read`). A texture of a buffer is `gpu_interop`'s.

use std::sync::Arc;

use wasmtime::bail;
use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};
use wasmtime_wasi::runtime::spawn_blocking;

use super::ProgramHardwareBuffer;
use crate::bindings::wasdk::platform::hardware_buffers::{
    self, HardwareBufferUsage, HostHardwareBuffer, HostHardwareBufferAllocator,
    HostHardwareBufferWithStore, ImageError, PixelBuffer, PixelFormat,
};
use crate::gpu::GpuInstance;
use crate::os::ALLOCATES;
use crate::running::RunningProgram;

/// `hardware-buffers.hardware-buffer-allocator`: the system's hardware buffer allocator, through
/// which the engine allocates the program's buffers.
pub struct HardwareBufferAllocator;

/// What the hardware buffers interface reads and writes in a program's store.
pub(crate) struct HardwareBuffersCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    /// The instance the program's devices are on, whose work drawing into a buffer a read of its
    /// pixels waits for.
    pub(crate) gpu: &'a GpuInstance,
    /// The program, whose stats count the bytes of its own buffers, and whose freeze holds what
    /// is submitted for it.
    pub(crate) program: &'a RunningProgram,
}

pub(crate) trait HardwareBuffersCtxView: Send {
    fn hardware_buffers_ctx(&mut self) -> HardwareBuffersCtx<'_>;
}

struct HasHardwareBuffers;

impl HasData for HasHardwareBuffers {
    type Data<'a> = HardwareBuffersCtx<'a>;
}

pub(crate) fn add_to_linker<T: HardwareBuffersCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    hardware_buffers::add_to_linker::<_, HasHardwareBuffers>(linker, T::hardware_buffers_ctx)
}

impl hardware_buffers::Host for HardwareBuffersCtx<'_> {
    /// The system's allocator, where the engine allocates buffers: on Apple's systems.
    fn get_hardware_buffer_allocator(
        &mut self,
    ) -> wasmtime::Result<Option<Resource<HardwareBufferAllocator>>> {
        if !ALLOCATES {
            return Ok(None);
        }
        Ok(Some(self.table.push(HardwareBufferAllocator)?))
    }
}

impl HostHardwareBufferAllocator for HardwareBuffersCtx<'_> {
    /// A buffer of the program's own; a side of no pixels is its mistake, and traps.
    fn allocate(
        &mut self,
        _allocator: Resource<HardwareBufferAllocator>,
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
    ) -> wasmtime::Result<Result<Resource<ProgramHardwareBuffer>, ImageError>> {
        if width == 0 || height == 0 {
            bail!(
                "the program allocated a hardware buffer of {width}x{height}, which holds no pixels"
            )
        }
        let stats = self.program.shared_stats();
        match ProgramHardwareBuffer::allocate(stats, width, height, format, usage) {
            Ok(buffer) => Ok(Ok(self.table.push(buffer)?)),
            Err(error) => Ok(Err(error)),
        }
    }

    fn drop(&mut self, allocator: Resource<HardwareBufferAllocator>) -> wasmtime::Result<()> {
        self.table.delete(allocator)?;
        Ok(())
    }
}

impl HostHardwareBuffer for HardwareBuffersCtx<'_> {
    fn width(&mut self, buffer: Resource<ProgramHardwareBuffer>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&buffer)?.width())
    }

    fn height(&mut self, buffer: Resource<ProgramHardwareBuffer>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&buffer)?.height())
    }

    fn usage(
        &mut self,
        buffer: Resource<ProgramHardwareBuffer>,
    ) -> wasmtime::Result<HardwareBufferUsage> {
        Ok(self.table.get(&buffer)?.usage())
    }

    /// The program lets go of the buffer; its textures, and a read, an encoding or a layer
    /// showing it, still hold it.
    fn drop(&mut self, buffer: Resource<ProgramHardwareBuffer>) -> wasmtime::Result<()> {
        self.table.delete(buffer)?;
        Ok(())
    }
}

impl<T: HardwareBuffersCtxView + 'static> HostHardwareBufferWithStore<T> for HasHardwareBuffers {
    /// The buffer's pixels copied out where they lie: on the program's thread for a buffer no
    /// texture was made of, which no GPU work of the program's drew into; on a thread of tokio's
    /// for blocking work otherwise, once the GPU has finished what the program submitted and
    /// wrote. `unsupported` for a buffer whose usage lacks `cpu-read`; a program reading one it
    /// allocated without asking for it is warned.
    async fn read_pixels(
        accessor: &Accessor<T, Self>,
        buffer: Resource<ProgramHardwareBuffer>,
    ) -> wasmtime::Result<Result<PixelBuffer, ImageError>> {
        let (buffer, lifecycle, gpu) = accessor.with(|mut access| {
            let context = access.get();
            let buffer = context.table.get(&buffer)?.clone();
            let program = context.program;
            if buffer.reads_unasked() {
                program.unasked_read().warn_once(program.name());
            }
            let lifecycle = program.lifecycle().clone();
            wasmtime::Result::<_>::Ok((buffer, lifecycle, Arc::clone(context.gpu)))
        })?;
        let work = buffer.hand_out(&gpu, &lifecycle).await;
        let pixels = if work.is_empty() {
            buffer.read_pixels()
        } else {
            let (read, waited) = (buffer.clone(), Arc::clone(&gpu));
            spawn_blocking(move || {
                work.wait(&waited);
                read.read_pixels()
            })
            .await
        };
        buffer.take_back(&gpu, &lifecycle).await;
        Ok(pixels)
    }
}
