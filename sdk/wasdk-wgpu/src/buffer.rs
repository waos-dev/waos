//! Buffers, and the copy that stands in for mapped memory.
//!
//! The engine shares no memory with the program, so a mapping's bytes cross the boundary once each
//! way, as Firefox's and Dawn's do with the memory they share with their GPU process: the mapped
//! range is copied in once, on the first range wgpu asks for (`read-mapped-range`), and what the
//! program wrote is copied back once, at `unmap` (`write-mapped-range`). The ranges wgpu hands out
//! are copies of that one, in the program's own memory. A buffer the GPU never writes, mapped for
//! writing with no usage but `COPY_SRC` beside it, as WebGPU allows, is never read back: its bytes
//! are the program's own last copy, kept between mappings, zeros at first, as wgpu-core makes
//! every buffer. A buffer made mapped holds zeros too, so its first mapping reads nothing either.
//!
//! The invariant: every range a mapping hands out lies in the copy, and is dropped, written into
//! it, before the buffer is unmapped, as wgpu requires of a mapped range.

use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wasdk::gpu::{Buffer, Device, TexelCopyBufferInfo};
use wgpu::custom::{
    BufferInterface, BufferMapCallback, BufferMappedRangeInterface, DispatchBuffer,
    DispatchBufferMappedRange,
};
use wgpu::{BufferAddress, BufferUsages, MapRangeError, WriteOnly};

use crate::convert;
use crate::runtime::PendingCallbacks;

/// The buffer behind a [`wgpu::Buffer`] from this backend.
#[derive(Debug)]
pub struct WasdkBuffer {
    buffer: Arc<Buffer>,
    /// Its size and usage, as made: what `Queue::write_buffer_with` checks a write against
    /// before handing out the memory to fill, as wgpu's backend for the web does.
    size: BufferAddress,
    usage: wgpu::BufferUsages,
    /// The callbacks its device waits to run, its mappings' among them.
    callbacks: PendingCallbacks,
    /// The program's copy of what is mapped, shared with the ranges handed out of it.
    mapped: Arc<Mutex<MappedCopy>>,
}

downcast!(WasdkBuffer: api wgpu::Buffer, dispatch DispatchBuffer);

impl WasdkBuffer {
    /// The engine's buffer behind the wgpu one.
    pub(crate) fn wit_buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Whether `size` bytes may be written at `offset` by the queue: the buffer takes copies,
    /// both are aligned to `COPY_BUFFER_ALIGNMENT`, and the write ends within the buffer.
    pub(crate) fn takes_write(&self, offset: BufferAddress, size: BufferAddress) -> bool {
        let aligned = |value: u64| value.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
        let within = offset.checked_add(size).is_some_and(|end| end <= self.size);
        self.usage.contains(wgpu::BufferUsages::COPY_DST)
            && aligned(offset)
            && aligned(size)
            && within
    }
}

pub(crate) fn create_buffer(
    device: &Device,
    callbacks: &PendingCallbacks,
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
        size: desc.size,
        usage: desc.usage,
        callbacks: callbacks.clone(),
        mapped: Arc::new(Mutex::new(MappedCopy::new(desc))),
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
        self.mapped().asked(range.clone());
        let mode = convert::map_mode_to_wit(mode);
        let size = range.end - range.start;
        let answer = self.buffer.map_async(mode, range.start, Some(size));
        self.callbacks.add_mapping(&self.buffer, answer, callback);
    }

    /// A copy of the range, out of the mapping's copy, which the first range asked copies in. A
    /// range that is not mapped traps the program, where wgpu-core answers a [`MapRangeError`],
    /// which a backend outside wgpu cannot make.
    fn get_mapped_range(
        &self,
        sub_range: Range<BufferAddress>,
    ) -> Result<DispatchBufferMappedRange, MapRangeError> {
        let data = self.mapped().range(&self.buffer, sub_range.clone());
        Ok(DispatchBufferMappedRange::custom(WasdkMappedRange {
            mapped: Arc::clone(&self.mapped),
            offset: sub_range.start,
            data,
            written: false,
        }))
    }

    /// Writes back what the program wrote, once, then unmaps.
    fn unmap(&self) {
        self.mapped().unmapping(&self.buffer);
        self.buffer.unmap();
        self.callbacks.run_resolved_mapping(&self.buffer);
    }

    fn destroy(&self) {
        self.buffer.destroy();
    }
}

impl WasdkBuffer {
    fn mapped(&self) -> MutexGuard<'_, MappedCopy> {
        lock(&self.mapped)
    }
}

fn lock(mapped: &Mutex<MappedCopy>) -> MutexGuard<'_, MappedCopy> {
    mapped.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The program's copy of a buffer's mapped bytes: what the ranges are copied out of, and what
/// they are written back into.
struct MappedCopy {
    /// Whether the bytes are kept between mappings, for a buffer the GPU never writes; else they
    /// are copied in once per mapping and let go at `unmap`.
    kept: bool,
    /// The buffer's size, which a kept copy is the whole of.
    size: BufferAddress,
    /// The range mapped, or asked for last.
    range: Range<BufferAddress>,
    /// The bytes, from `at` in the buffer; none until a range is asked of a mapping.
    bytes: Option<Vec<u8>>,
    at: BufferAddress,
    /// The span of the buffer written since it was mapped, which `unmap` writes back.
    written: Option<Range<BufferAddress>>,
}

impl fmt::Debug for MappedCopy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MappedCopy")
            .field("kept", &self.kept)
            .field("range", &self.range)
            .field("copied", &self.bytes.as_ref().map(Vec::len))
            .field("written", &self.written)
            .finish()
    }
}

impl MappedCopy {
    /// The copy of a buffer made as `desc` says: zeros, mapped whole, where it is made mapped.
    fn new(desc: &wgpu::BufferDescriptor<'_>) -> MappedCopy {
        let gpu_never_writes = desc.usage.contains(BufferUsages::MAP_WRITE)
            && (BufferUsages::MAP_WRITE | BufferUsages::COPY_SRC).contains(desc.usage);
        MappedCopy {
            kept: gpu_never_writes,
            size: desc.size,
            range: 0..desc.size,
            bytes: desc.mapped_at_creation.then(|| zeros(desc.size)),
            at: 0,
            written: None,
        }
    }

    /// A mapping of `range` is asked for: a copy not kept is copied in afresh.
    fn asked(&mut self, range: Range<BufferAddress>) {
        self.range = range;
        if !self.kept {
            self.bytes = None;
        }
        self.written = None;
    }

    /// A copy of `sub_range` of the mapping of `buffer`, its bytes copied in first where this is
    /// the mapping's first range: read from the engine, or zeros for a kept copy never mapped.
    fn range(&mut self, buffer: &Buffer, sub_range: Range<BufferAddress>) -> Vec<u8> {
        assert!(
            self.range.start <= sub_range.start && sub_range.end <= self.range.end,
            "wasdk-wgpu: a mapped range {sub_range:?} outside the mapping, {:?}",
            self.range
        );
        let bytes = match &mut self.bytes {
            Some(bytes) => bytes,
            None if self.kept => {
                self.at = 0;
                self.bytes.insert(zeros(self.size))
            }
            None => {
                let size = self.range.end - self.range.start;
                self.at = self.range.start;
                self.bytes
                    .insert(buffer.read_mapped_range(self.range.start, size))
            }
        };
        bytes[index(sub_range.start - self.at)..index(sub_range.end - self.at)].to_vec()
    }

    /// `data`, a range's bytes, written at `offset` in the buffer.
    fn write(&mut self, offset: BufferAddress, data: &[u8]) {
        let Some(bytes) = &mut self.bytes else {
            return;
        };
        let start = index(offset - self.at);
        bytes[start..start + data.len()].copy_from_slice(data);
        let end = offset + data.len() as u64;
        self.written = Some(match self.written.take() {
            Some(written) => written.start.min(offset)..written.end.max(end),
            None => offset..end,
        });
    }

    /// The buffer is about to be unmapped: what was written goes back to `buffer`, once, and a
    /// copy not kept goes.
    fn unmapping(&mut self, buffer: &Buffer) {
        if let (Some(written), Some(bytes)) = (self.written.take(), &self.bytes) {
            let span = &bytes[index(written.start - self.at)..index(written.end - self.at)];
            buffer.write_mapped_range(written.start, span);
        }
        if !self.kept {
            self.bytes = None;
        }
    }
}

/// `offset` as an index into the program's memory.
fn index(offset: BufferAddress) -> usize {
    usize::try_from(offset).expect("a mapped range fits the program's memory")
}

/// `size` zeros, what wgpu-core makes a buffer hold.
fn zeros(size: BufferAddress) -> Vec<u8> {
    vec![0; index(size)]
}

/// A copy of a mapped range, out of the mapping's copy. A range handed out for writing is written
/// into it when it is dropped, which must happen before the buffer is unmapped.
#[derive(Debug)]
pub struct WasdkMappedRange {
    mapped: Arc<Mutex<MappedCopy>>,
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
            lock(&self.mapped).write(self.offset, &self.data);
        }
    }
}
