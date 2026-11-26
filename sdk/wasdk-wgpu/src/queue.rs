//! The queue: writes, submissions, and the staging buffer `write_buffer_with` fills.

use std::sync::Arc;

use wasdk::gpu::Queue;
use wgpu::custom::{
    BoxSubmittedWorkDoneCallback, DispatchBlas, DispatchBuffer, DispatchCommandBuffer,
    DispatchQueueWriteBuffer, DispatchSurfaceOutputDetail, QueueInterface,
    QueueWriteBufferInterface,
};
use wgpu::{BufferAddress, BufferSize, WriteOnly};

use crate::buffer::WasdkBuffer;
use crate::command::WasdkCommandBuffer;
use crate::convert;
use crate::left_out::left_out;
use crate::runtime::PendingCallbacks;
use crate::surface::WasdkSurfaceOutputDetail;
use crate::texture::texel_copy_texture_to_wit;

/// The queue behind a [`wgpu::Queue`] from this backend.
#[derive(Debug)]
pub struct WasdkQueue {
    queue: Arc<Queue>,
    /// The callbacks its device waits to run, its submitted work's among them.
    callbacks: PendingCallbacks,
}

impl WasdkQueue {
    pub(crate) fn new(queue: Queue, callbacks: PendingCallbacks) -> Self {
        WasdkQueue {
            queue: Arc::new(queue),
            callbacks,
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
        left_out("Queue::copy_external_image_to_texture (a browser's image)")
    }

    fn write_buffer(&self, buffer: &DispatchBuffer, offset: BufferAddress, data: &[u8]) {
        let buffer = WasdkBuffer::of_dispatch(buffer).wit_buffer();
        self.queue.write_buffer(buffer, offset, data);
    }

    fn create_staging_buffer(&self, size: BufferSize) -> Option<DispatchQueueWriteBuffer> {
        Some(DispatchQueueWriteBuffer::custom(WasdkStagingBuffer::new(
            size,
        )))
    }

    /// Checked here, as wgpu's backend for the web checks it, against the buffer's size and
    /// usage as made: a write the engine would refuse answers `None` before the program fills
    /// memory for it.
    fn validate_write_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: BufferAddress,
        size: BufferSize,
    ) -> Option<()> {
        WasdkBuffer::of_dispatch(buffer)
            .takes_write(offset, size.get())
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
        self.queue.write_texture(
            &texel_copy_texture_to_wit(&texture),
            data,
            convert::texel_copy_layout_to_wit(data_layout),
            convert::extent_to_wit(size),
        );
    }

    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let command_buffers = command_buffers
            .map(|buffer| WasdkCommandBuffer::of_dispatch(&buffer).take())
            .collect();
        self.queue.submit(command_buffers)
    }

    fn get_timestamp_period(&self) -> f32 {
        self.queue.timestamp_period()
    }

    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        let answer = self.queue.on_submitted_work_done();
        self.callbacks.add_submitted_work_done(answer, callback);
    }

    fn compact_blas(&self, _blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        left_out("ray tracing (Queue::compact_blas)")
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
