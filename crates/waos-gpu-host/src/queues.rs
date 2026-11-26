//! The queue: writes before the next submission, submissions through the embedder's gate, and
//! the wait for work submitted.
//!
//! The invariant: a submission starts only inside the gate's `while_allowed`, so none starts
//! once the embedder holds the program's submissions.

use futures::channel::oneshot;
use wasmtime::component::{Access, Accessor, Resource};

use crate::buffers::Buffer;
use crate::commands::CommandBuffer;
use crate::devices::{self, Device};
use crate::textures::texel_copy_texture;
use crate::wasdk::gpu::gpu::{
    self, Extent3d, SubmissionIndex, TexelCopyBufferLayout, TexelCopyTextureInfo,
};
use crate::{GpuCtx, HasGpuCtx};

/// A device's queue as the program holds it: the device it is of, whose errors it reports to.
pub struct Queue {
    device: Device,
}

impl Queue {
    pub(crate) fn of(device: &Device) -> Queue {
        Queue {
            device: device.clone(),
        }
    }
}

impl GpuCtx<'_> {
    /// Submits `command_buffers` on `queue` now, giving them back once submitted.
    fn submit(
        &mut self,
        queue: &Resource<Queue>,
        command_buffers: Vec<CommandBuffer>,
    ) -> wasmtime::Result<SubmissionIndex> {
        let ids: Vec<_> = command_buffers.iter().map(|buffer| buffer.id()).collect();
        let queue = self.table.get(queue)?;
        let index = match self.instance.queue_submit(queue.device.queue_id(), &ids) {
            Ok(index) => index,
            Err((index, error)) => {
                queue
                    .device
                    .errors
                    .report(Some(error), "Queue::submit", None);
                index
            }
        };
        Ok(index)
    }
}

impl gpu::HostQueue for GpuCtx<'_> {
    fn write_buffer(
        &mut self,
        queue: Resource<Queue>,
        buffer: Resource<Buffer>,
        offset: u64,
        data: Vec<u8>,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get(&buffer)?.id();
        let queue = self.table.get(&queue)?;
        let written =
            self.instance
                .queue_write_buffer(queue.device.queue_id(), buffer, offset, &data);
        queue
            .device
            .errors
            .report(written.err(), "Queue::write_buffer", None);
        Ok(())
    }

    fn write_texture(
        &mut self,
        queue: Resource<Queue>,
        destination: TexelCopyTextureInfo,
        data: Vec<u8>,
        layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) -> wasmtime::Result<()> {
        let destination = texel_copy_texture(self.table, destination)?;
        let queue = self.table.get(&queue)?;
        let written = self.instance.queue_write_texture(
            queue.device.queue_id(),
            &destination,
            &data,
            &layout.into(),
            &size.into(),
        );
        queue
            .device
            .errors
            .report(written.err(), "Queue::write_texture", None);
        Ok(())
    }

    fn timestamp_period(&mut self, queue: Resource<Queue>) -> wasmtime::Result<f32> {
        let queue = self.table.get(&queue)?;
        Ok(self
            .instance
            .queue_get_timestamp_period(queue.device.queue_id()))
    }

    fn drop(&mut self, queue: Resource<Queue>) -> wasmtime::Result<()> {
        self.table.delete(queue)?;
        Ok(())
    }
}

impl<T: Send> gpu::HostQueueWithStore<T> for HasGpuCtx {
    /// Waits while the embedder holds submissions, then submits inside the gate, so a hold that
    /// came meanwhile refuses the submission and the wait starts again. The program's store waits
    /// with it: to the program this is a plain call.
    async fn submit(
        mut host: Access<'_, T, Self>,
        queue: Resource<Queue>,
        command_buffers: Vec<Resource<CommandBuffer>>,
    ) -> wasmtime::Result<SubmissionIndex> {
        let command_buffers = {
            let ctx = host.get();
            command_buffers
                .into_iter()
                .map(|buffer| Ok(ctx.table.delete(buffer)?))
                .collect::<wasmtime::Result<Vec<_>>>()?
        };
        let mut command_buffers = Some(command_buffers);
        loop {
            let allowed = host.get().submissions.map(|gate| gate.until_allowed());
            if let Some(allowed) = allowed {
                allowed.await;
            }
            let mut ctx = host.get();
            let Some(gate) = ctx.submissions else {
                return ctx.submit(&queue, command_buffers.take().unwrap_or_default());
            };
            let mut submitted = None;
            gate.while_allowed(&mut || {
                submitted = Some(ctx.submit(&queue, command_buffers.take().unwrap_or_default()));
            });
            if let Some(submitted) = submitted {
                return submitted;
            }
        }
    }

    async fn on_submitted_work_done(
        accessor: &Accessor<T, Self>,
        queue: Resource<Queue>,
    ) -> wasmtime::Result<()> {
        let (device, done) = accessor.with(|mut access| -> wasmtime::Result<_> {
            let ctx = access.get();
            let queue = ctx.table.get(&queue)?;
            let (answer, done) = oneshot::channel();
            ctx.instance.queue_on_submitted_work_done(
                queue.device.queue_id(),
                Box::new(move || {
                    // A program that stopped waiting dropped the other end.
                    let _ = answer.send(());
                }),
            );
            Ok((queue.device.clone(), done))
        })?;
        let polled = devices::poll(device, wgpu_types::PollType::wait_indefinitely()).await;
        if let Err(error) = polled {
            tracing::warn!("waiting for a queue's submitted work: {error}");
        }
        // The wait ran the callback, or the device is lost and dropped it.
        let _ = done.await;
        Ok(())
    }
}
