//! Buffers, and the copies that stand in for mapped memory.
//!
//! The engine shares no memory with the program, so a mapped range is copied in when wgpu asks
//! for it (`read-mapped-range`) and, where it was written, copied back when it is dropped
//! (`write-mapped-range`), which must happen before the buffer is unmapped.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wasdk::gpu::{Buffer, Device, TexelCopyBufferInfo};
use wgpu::custom::{
    BufferInterface, BufferMapCallback, BufferMappedRangeInterface, DispatchBuffer,
    DispatchBufferMappedRange,
};
use wgpu::{BufferAddress, MapRangeError, WriteOnly};

use crate::convert;
use crate::runtime::{Owed, OwedCallbacks};

/// The buffer behind a [`wgpu::Buffer`] from this backend.
#[derive(Debug)]
pub struct WasdkBuffer {
    buffer: Arc<Buffer>,
    /// The callbacks its device owes, a mapping's among them.
    owed: OwedCallbacks,
    /// Whether it was made mapped and no range of it was handed out yet: it holds zeros, as
    /// wgpu-core makes every buffer, so its first range is not copied from the engine.
    untouched_since_creation: AtomicBool,
}

downcast!(WasdkBuffer: api wgpu::Buffer, dispatch DispatchBuffer);

impl WasdkBuffer {
    /// The engine's buffer behind the wgpu one.
    pub(crate) fn wit_buffer(&self) -> &Buffer {
        &self.buffer
    }
}

pub(crate) fn create_buffer(
    device: &Device,
    owed: &OwedCallbacks,
    desc: &wgpu::BufferDescriptor<'_>,
) -> DispatchBuffer {
    let descriptor = wasdk::gpu_types::BufferDescriptor {
        label: convert::label(desc.label),
        size: desc.size,
        usage: convert::buffer_usages_to_wit(desc.usage),
        mapped_at_creation: desc.mapped_at_creation,
    };
    DispatchBuffer::custom(WasdkBuffer {
        buffer: Arc::new(device.create_buffer(&descriptor)),
        owed: owed.clone(),
        untouched_since_creation: AtomicBool::new(desc.mapped_at_creation),
    })
}

pub(crate) fn texel_copy_buffer_to_wit<'a>(
    info: &wgpu::TexelCopyBufferInfo<'a>,
) -> TexelCopyBufferInfo<'a> {
    TexelCopyBufferInfo {
        buffer: WasdkBuffer::of(info.buffer).wit_buffer(),
        layout: convert::texel_copy_layout_to_wit(info.layout),
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
            mode: convert::map_mode_to_wit(mode),
            offset: range.start,
            size: range.end - range.start,
            callback,
        });
    }

    /// The range copied in, except the first range of a buffer made mapped, which is zeros the
    /// program makes itself. A range that is not mapped traps the program in the engine, where
    /// wgpu-core answers a [`MapRangeError`], which a backend outside wgpu cannot make.
    fn get_mapped_range(
        &self,
        sub_range: Range<BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, MapRangeError> {
        let size = sub_range.end - sub_range.start;
        let data = if self.untouched_since_creation.swap(false, Ordering::Relaxed) {
            zeros(size)
        } else {
            self.buffer.read_mapped_range(sub_range.start, size)
        };
        Ok(DispatchBufferMappedRange::custom(WasdkMappedRange {
            buffer: Arc::clone(&self.buffer),
            offset: sub_range.start,
            data,
            written: false,
        }))
    }

    fn unmap(&self) {
        // What the GPU writes into it from now on is no longer zeros.
        self.untouched_since_creation
            .store(false, Ordering::Relaxed);
        self.buffer.unmap();
    }

    fn destroy(&self) {
        self.buffer.destroy();
    }
}

/// `size` zeros, what a buffer made mapped holds.
fn zeros(size: BufferAddress) -> Vec<u8> {
    let size = usize::try_from(size).expect("a mapped range fits the program's memory");
    vec![0; size]
}

/// A copy of a mapped range. A range handed out for writing is copied back when it is dropped,
/// which must happen before the buffer is unmapped.
#[derive(Debug)]
pub struct WasdkMappedRange {
    buffer: Arc<Buffer>,
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
        crate::left_out::left_out("BufferView::as_uint8array (a JavaScript typed array)")
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
        if self.written {
            self.buffer.write_mapped_range(self.offset, &self.data);
        }
    }
}
