//! Buffers, and the copies that stand in for mapped memory.

use std::ops::Range;
use std::sync::Arc;

use wasdk::webgpu::{GpuBuffer, GpuBufferDescriptor, GpuDevice, GpuTexelCopyBufferInfo};
use wgpu::custom::{
    BufferInterface, BufferMapCallback, BufferMappedRangeInterface, DispatchBuffer,
    DispatchBufferMappedRange,
};
use wgpu::{BufferAddress, MapRangeError, WriteOnly};

use crate::convert;
use crate::runtime::{Owed, OwedCallbacks};

#[derive(Debug)]
pub struct WasdkBuffer {
    buffer: Arc<GpuBuffer>,
    /// The callbacks its device owes, a mapping's among them.
    owed: OwedCallbacks,
}

downcast!(WasdkBuffer: api wgpu::Buffer, dispatch DispatchBuffer);

impl WasdkBuffer {
    pub(crate) fn gpu_buffer(&self) -> &GpuBuffer {
        &self.buffer
    }
}

pub(crate) fn create_buffer(
    device: &GpuDevice,
    owed: &OwedCallbacks,
    desc: &wgpu::BufferDescriptor<'_>,
) -> DispatchBuffer {
    let descriptor = GpuBufferDescriptor {
        size: desc.size,
        usage: convert::buffer_usages_to_wasi(desc.usage),
        mapped_at_creation: Some(desc.mapped_at_creation),
        label: convert::label(desc.label),
    };
    DispatchBuffer::custom(WasdkBuffer {
        buffer: Arc::new(device.create_buffer(&descriptor)),
        owed: owed.clone(),
    })
}

pub(crate) fn texel_copy_buffer_to_wasi<'a>(
    info: &wgpu::TexelCopyBufferInfo<'a>,
) -> GpuTexelCopyBufferInfo<'a> {
    GpuTexelCopyBufferInfo {
        buffer: WasdkBuffer::of(info.buffer).gpu_buffer(),
        offset: Some(info.layout.offset),
        bytes_per_row: info.layout.bytes_per_row,
        rows_per_image: info.layout.rows_per_image,
    }
}

impl BufferInterface for WasdkBuffer {
    fn map_async(
        &self,
        mode: wgpu::MapMode,
        range: Range<BufferAddress>,
        callback: BufferMapCallback,
    ) {
        self.owed.owe(Owed::Mapping {
            buffer: Arc::clone(&self.buffer),
            mode: convert::map_mode_to_wasi(mode),
            offset: range.start,
            size: range.end - range.start,
            callback,
        });
    }

    /// The host cannot share the mapping, so the range is copied out; the only errors are
    /// misuse (an unmapped buffer or a range outside the mapping), which wgpu's
    /// [`MapRangeError`] cannot carry from outside wgpu, so they panic with the host's message.
    fn get_mapped_range(
        &self,
        sub_range: Range<BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, MapRangeError> {
        let data = self
            .buffer
            .get_mapped_range_get_with_copy(
                Some(sub_range.start),
                Some(sub_range.end - sub_range.start),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "wasdk-wgpu: Buffer::get_mapped_range({sub_range:?}): {}",
                    error.message
                )
            });
        Ok(DispatchBufferMappedRange::custom(WasdkMappedRange {
            buffer: Arc::clone(&self.buffer),
            offset: sub_range.start,
            data,
            written: false,
        }))
    }

    fn unmap(&self) {
        self.buffer
            .unmap()
            .unwrap_or_else(|error| panic!("wasdk-wgpu: Buffer::unmap: {}", error.message));
    }

    fn destroy(&self) {
        self.buffer.destroy();
    }
}

/// A copy of a mapped range. A range handed out for writing is copied back when it is dropped,
/// which must happen before the buffer is unmapped.
#[derive(Debug)]
pub struct WasdkMappedRange {
    buffer: Arc<GpuBuffer>,
    offset: BufferAddress,
    data: Vec<u8>,
    written: bool,
}

// `read_slice` and `write_slice` are `unsafe` in the trait because a wgpu-core mapping is raw
// memory; here they hand out a `Vec` this type owns.
#[allow(unsafe_code)]
impl BufferMappedRangeInterface for WasdkMappedRange {
    #[cfg(target_arch = "wasm32")]
    fn as_uint8array(&self) -> &js_sys::Uint8Array {
        crate::gap::wasi_gap("BufferViewMut::as_uint8array (a JavaScript typed array)")
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    unsafe fn read_slice(&self) -> &[u8] {
        &self.data
    }

    unsafe fn write_slice(&mut self) -> WriteOnly<'_, [u8]> {
        self.written = true;
        WriteOnly::from(self.data.as_mut_slice())
    }
}

impl Drop for WasdkMappedRange {
    fn drop(&mut self) {
        if !self.written {
            return;
        }
        self.buffer
            .get_mapped_range_set_with_copy(
                &self.data,
                Some(self.offset),
                Some(self.data.len() as u64),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "wasdk-wgpu: writing a mapped range back at {}: {}",
                    self.offset, error.message
                )
            });
    }
}
