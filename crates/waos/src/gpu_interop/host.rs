//! The code serving `wasdk:platform/gpu-interop` to a program: a buffer it holds imported as a
//! texture on its device, the WebGPU host's own `gpu-texture`, for what the buffer's usage allows.

use waos_webgpu_host::{Device, Texture};
use wasmtime::component::{HasData, Linker, Resource, ResourceTable};

use super::texture_usages;
use crate::bindings::wasdk::platform::gpu_interop;
use crate::gpu::GpuInstance;
use crate::hardware_buffers::ProgramHardwareBuffer;

/// What the GPU interop interface reads and writes in a program's store.
pub(crate) struct GpuInteropCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    /// The instance the program's devices are on, which a buffer's texture is made on.
    pub(crate) gpu: &'a GpuInstance,
}

pub(crate) trait GpuInteropCtxView: Send {
    fn gpu_interop_ctx(&mut self) -> GpuInteropCtx<'_>;
}

struct HasGpuInterop;

impl HasData for HasGpuInterop {
    type Data<'a> = GpuInteropCtx<'a>;
}

pub(crate) fn add_to_linker<T: GpuInteropCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    gpu_interop::add_to_linker::<_, HasGpuInterop>(linker, T::gpu_interop_ctx)
}

impl gpu_interop::Host for GpuInteropCtx<'_> {
    /// A texture over the buffer's memory on `device`, holding the buffer until the GPU is done
    /// with it. A failure the system answers is the engine's, and traps the program.
    fn import_texture(
        &mut self,
        device: Resource<Device>,
        buffer: Resource<ProgramHardwareBuffer>,
    ) -> wasmtime::Result<Resource<Texture>> {
        let buffer = self.table.get(&buffer)?;
        let device = self.table.get(&device)?;
        let texture = buffer
            .texture_on(self.gpu, device, texture_usages(buffer.usage()))
            .map_err(wasmtime::Error::from_anyhow)?;
        Ok(self.table.push(texture)?)
    }
}
