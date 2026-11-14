//! The queue: writes, submissions, and the staging buffer `write_buffer_with` fills.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wasdk::webgpu::{GpuCommandBuffer, GpuQueue};
use wgpu::custom::{
    BoxSubmittedWorkDoneCallback, DispatchBlas, DispatchBuffer, DispatchCommandBuffer,
    DispatchQueueWriteBuffer, DispatchSurfaceOutputDetail, QueueInterface,
    QueueWriteBufferInterface,
};
use wgpu::{BufferAddress, BufferSize, BufferUsages, WriteOnly};

use crate::buffer::WasdkBuffer;
use crate::command::WasdkCommandBuffer;
use crate::gap::wasi_gap;
use crate::surface::WasdkSurfaceOutputDetail;
use crate::texture::texel_copy_texture_to_wasi;
use crate::{convert, runtime};

#[derive(Debug)]
pub struct WasdkQueue {
    queue: Arc<GpuQueue>,
    /// Submissions so far: the index wgpu hands back for each.
    submissions: AtomicU64,
}

impl WasdkQueue {
    pub(crate) fn new(queue: GpuQueue) -> Self {
        WasdkQueue {
            queue: Arc::new(queue),
            submissions: AtomicU64::new(0),
        }
    }
}

impl QueueInterface for WasdkQueue {
    #[cfg(target_arch = "wasm32")]
    fn copy_external_image_to_texture(
        &self,
        _source: &wgpu::CopyExternalImageSourceInfo,
        _dest: wgpu::CopyExternalImageDestInfo<&wgpu::Texture>,
        _size: wgpu::Extent3d,
    ) {
        wasi_gap("Queue::copy_external_image_to_texture (a browser image source)")
    }

    fn write_buffer(&self, buffer: &DispatchBuffer, offset: BufferAddress, data: &[u8]) {
        let buffer = WasdkBuffer::of_dispatch(buffer).gpu_buffer();
        self.queue
            .write_buffer_with_copy(buffer, offset, data, None, None)
            .unwrap_or_else(|error| panic!("wasdk-wgpu: Queue::write_buffer: {}", error.message));
    }

    fn create_staging_buffer(&self, size: BufferSize) -> Option<DispatchQueueWriteBuffer> {
        Some(DispatchQueueWriteBuffer::custom(WasdkStagingBuffer::new(
            size,
        )))
    }

    fn validate_write_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: BufferAddress,
        size: BufferSize,
    ) -> Option<()> {
        let buffer = WasdkBuffer::of_dispatch(buffer).gpu_buffer();
        let usages = convert::buffer_usages_from_wasi(buffer.usage());
        let size = size.get();
        let aligned = |value: u64| value.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
        let in_bounds = offset
            .checked_add(size)
            .is_some_and(|end| end <= buffer.size());
        (usages.contains(BufferUsages::COPY_DST) && aligned(size) && aligned(offset) && in_bounds)
            .then_some(())
    }

    fn write_staging_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: BufferAddress,
        staging_buffer: &DispatchQueueWriteBuffer,
    ) {
        let staging = WasdkStagingBuffer::of_dispatch(staging_buffer);
        QueueInterface::write_buffer(self, buffer, offset, &staging.data);
    }

    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        self.queue.write_texture_with_copy(
            &texel_copy_texture_to_wasi(&texture),
            data,
            convert::texel_copy_layout_to_wasi(data_layout),
            convert::extent_to_wasi(size),
        );
    }

    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let buffers: Vec<DispatchCommandBuffer> = command_buffers.collect();
        let wasi_buffers: Vec<&GpuCommandBuffer> = buffers
            .iter()
            .map(|buffer| WasdkCommandBuffer::of_dispatch(buffer).gpu_command_buffer())
            .collect();
        self.queue.submit(&wasi_buffers);
        self.submissions.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn get_timestamp_period(&self) -> f32 {
        // WebGPU timestamps are in nanoseconds.
        1.0
    }

    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        let queue = Arc::clone(&self.queue);
        runtime::spawn(async move {
            queue.on_submitted_work_done().await;
            callback();
        });
    }

    fn compact_blas(&self, _blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        wasi_gap("ray tracing (Queue::compact_blas)")
    }

    /// Presents the frame on its view, with the layers the view holds beneath it.
    fn present(&self, detail: &DispatchSurfaceOutputDetail) {
        WasdkSurfaceOutputDetail::of_dispatch(detail).present();
    }
}

/// The program's own memory, which `Queue::write_buffer_with` fills before it is written in
/// one copy.
#[derive(Debug)]
pub struct WasdkStagingBuffer {
    data: Box<[u8]>,
}

downcast!(WasdkStagingBuffer: dispatch DispatchQueueWriteBuffer);

impl WasdkStagingBuffer {
    fn new(size: BufferSize) -> Self {
        WasdkStagingBuffer {
            data: vec![0; size.get() as usize].into_boxed_slice(),
        }
    }
}

// `write_slice` is `unsafe` in the trait; here it hands out the boxed slice this type owns.
#[allow(unsafe_code)]
impl QueueWriteBufferInterface for WasdkStagingBuffer {
    fn len(&self) -> usize {
        self.data.len()
    }

    unsafe fn write_slice(&mut self) -> WriteOnly<'_, [u8]> {
        WriteOnly::from(&mut *self.data)
    }
}
