//! The buffer, and its mapping: a range mapped for the CPU once the GPU is done with it, copied
//! out of the program's memory and into it, since a component shares no memory with the engine.
//!
//! The invariant: the one `unsafe` here reads or writes a mapped range through the pointer
//! wgpu-core answers for it, within the length wgpu-core answers, while the buffer is held, so
//! the mapping cannot go meanwhile.

use std::sync::Arc;

use futures::channel::oneshot;
use wasmtime::component::{Accessor, Resource};
use wgpu_core::id::BufferId;

use crate::devices::{self, Device};
use crate::wasdk::gpu::gpu::{self, MapMode};
use crate::{GpuCtx, GpuObject, HasGpuCtx};

/// A buffer as the program holds it, with the device its errors and its mapping's wait are of.
pub struct Buffer {
    object: Arc<GpuObject<BufferId>>,
    device: Device,
}

impl Buffer {
    /// The buffer `descriptor` describes, on `device`.
    pub(crate) fn create(
        device: &Device,
        descriptor: wgpu_core::resource::BufferDescriptor<'static>,
    ) -> Buffer {
        let instance = device.instance();
        let (buffer, error) = instance.device_create_buffer(device.device_id(), &descriptor, None);
        device
            .errors
            .report(error, "Device::create_buffer", descriptor.label.as_deref());
        let object = Arc::new(GpuObject::new(buffer, instance));
        let mappable = descriptor.mapped_at_creation
            || descriptor.usage.intersects(
                wgpu_types::BufferUsages::MAP_READ | wgpu_types::BufferUsages::MAP_WRITE,
            );
        if mappable {
            device.add_mappable_buffer(&object);
        }
        Buffer {
            object,
            device: device.clone(),
        }
    }

    pub(crate) fn id(&self) -> BufferId {
        self.object.id()
    }

    /// The mapped range at `offset` of `size` bytes, as wgpu-core answers it, or the trap for a
    /// range not mapped, as wgpu panics there.
    fn mapped_range(
        &self,
        offset: u64,
        size: u64,
    ) -> wasmtime::Result<(std::ptr::NonNull<u8>, usize)> {
        let (pointer, length) = self
            .object
            .instance()
            .buffer_get_mapped_range(self.id(), offset, Some(size))
            .map_err(|error| wasmtime::format_err!("Buffer::get_mapped_range: {error}"))?;
        Ok((pointer, usize::try_from(length)?))
    }
}

impl gpu::HostBuffer for GpuCtx<'_> {
    fn read_mapped_range(
        &mut self,
        buffer: Resource<Buffer>,
        offset: u64,
        size: u64,
    ) -> wasmtime::Result<Vec<u8>> {
        let (pointer, length) = self.table.get(&buffer)?.mapped_range(offset, size)?;
        // SAFETY: wgpu-core answered the range as mapped, `length` bytes from `pointer`, and the
        // buffer is held for the copy (the module's invariant).
        #[allow(unsafe_code)]
        let mapped = unsafe { std::slice::from_raw_parts(pointer.as_ptr(), length) };
        Ok(mapped.to_vec())
    }

    fn write_mapped_range(
        &mut self,
        buffer: Resource<Buffer>,
        offset: u64,
        data: Vec<u8>,
    ) -> wasmtime::Result<()> {
        let (pointer, length) = self
            .table
            .get(&buffer)?
            .mapped_range(offset, u64::try_from(data.len())?)?;
        // SAFETY: as in `read_mapped_range`; nothing else reaches the mapping while the program's
        // call runs.
        #[allow(unsafe_code)]
        let mapped = unsafe { std::slice::from_raw_parts_mut(pointer.as_ptr(), length) };
        let written = length.min(data.len());
        mapped[..written].copy_from_slice(&data[..written]);
        Ok(())
    }

    fn unmap(&mut self, buffer: Resource<Buffer>) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?;
        let unmapped = self.instance.buffer_unmap(buffer.id());
        buffer
            .device
            .errors
            .report(unmapped.err(), "Buffer::buffer_unmap", None);
        Ok(())
    }

    fn destroy(&mut self, buffer: Resource<Buffer>) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        self.instance.buffer_destroy(buffer);
        Ok(())
    }

    fn drop(&mut self, buffer: Resource<Buffer>) -> wasmtime::Result<()> {
        self.table.delete(buffer)?;
        Ok(())
    }
}

impl<T: Send> gpu::HostBufferWithStore<T> for HasGpuCtx {
    /// Asks wgpu-core for the mapping, then waits on the device until its callback has answered.
    async fn map_async(
        accessor: &Accessor<T, Self>,
        buffer: Resource<Buffer>,
        mode: MapMode,
        offset: u64,
        size: Option<u64>,
    ) -> wasmtime::Result<Result<(), String>> {
        let (device, mapped) = accessor.with(|mut access| -> wasmtime::Result<_> {
            let ctx = access.get();
            let buffer = ctx.table.get(&buffer)?;
            let (answer, mapped) = oneshot::channel();
            let operation = wgpu_core::resource::BufferMapOperation {
                host: mode.into(),
                callback: Some(Box::new(move |result| {
                    // A program that stopped waiting dropped the other end.
                    let _ = answer.send(result);
                })),
            };
            let asked = ctx
                .instance
                .buffer_map_async(buffer.id(), offset, size, operation);
            buffer
                .device
                .errors
                .report(asked.err(), "Buffer::map_async", None);
            Ok((buffer.device.clone(), mapped))
        })?;
        let polled = devices::poll(device, wgpu_types::PollType::wait_indefinitely()).await;
        if let Err(error) = polled {
            tracing::warn!("waiting for a buffer's mapping: {error}");
        }
        // The wait ran the callback; a device lost meanwhile dropped it unanswered.
        Ok(match mapped.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(wgpu_core::resource::BufferAccessError::MapAborted.to_string()),
        })
    }
}
