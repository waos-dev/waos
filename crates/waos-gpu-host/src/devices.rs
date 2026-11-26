//! The device: what it makes, its error scopes and uncaptured errors, its loss, and the waits for
//! its work. A device with its queue, adapter, errors and loss is shared by every handle on it:
//! the program's own, its queue's, a surface configured with it, and any the embedder keeps for
//! work it finishes on the program's behalf. The last to go drops the device, its queue and its
//! adapter.
//!
//! The invariant: a buffer that can be mapped, one made with a map usage or mapped at creation,
//! is among its device's mappable buffers for as long as the program holds it, so `destroy`
//! unmaps it before the device is lost, as wgpu-core does not.

use std::sync::{Arc, Mutex, Weak};

use futures::{FutureExt, StreamExt};
use wasmtime::StoreContextMut;
use wasmtime::component::{
    Access, Accessor, Destination, FutureReader, Resource, StreamProducer, StreamReader,
    StreamResult,
};
use wasmtime_wasi::runtime::spawn_blocking;
use wgpu_core::global::Global;
use wgpu_core::id::{AdapterId, BufferId, DeviceId, QueueId};

use crate::adapters::Adapter;
use crate::binding::{BindGroup, BindGroupLayout, PipelineLayout};
use crate::buffers::Buffer;
use crate::commands::{CommandEncoder, QuerySet};
use crate::conversions::{external_texture_descriptor, features_of, label};
use crate::device_loss::{DeviceLoss, LostInfo};
use crate::errors::ErrorSink;
use crate::pipelines::{ComputePipeline, RenderPipeline};
use crate::render_bundles::RenderBundleEncoder;
use crate::shaders::ShaderModule;
use crate::textures::{ExternalTexture, Sampler, Texture, TextureView};
use crate::wasdk::gpu::gpu::{
    self, AdapterInfo, BindGroupDescriptor, BindGroupLayoutDescriptor, BufferDescriptor,
    ComputePipelineDescriptor, DeviceLostInfo, Error, ErrorFilter, ExternalTextureDescriptor,
    Feature, PipelineLayoutDescriptor, PollError, PollStatus, PollType, QuerySetDescriptor,
    RenderBundleEncoderDescriptor, RenderPipelineDescriptor, SamplerDescriptor,
    ShaderModuleDescriptor, TextureDescriptor,
};
use crate::{GpuCtx, GpuObject, HasGpuCtx};

/// A device and what every handle on it shares.
#[derive(Clone)]
pub struct Device {
    device: Arc<GpuObject<DeviceId>>,
    queue: Arc<GpuObject<QueueId>>,
    adapter: Adapter,
    pub(crate) errors: Arc<ErrorSink>,
    loss: Arc<DeviceLoss>,
    mappable_buffers: Arc<MappableBuffers>,
}

impl Device {
    /// The device wgpu-core opened as `device` and `queue` on `adapter`, its loss told by wgpu-core.
    pub(crate) fn opened(
        instance: &Arc<Global>,
        adapter: Adapter,
        device: DeviceId,
        queue: QueueId,
    ) -> Device {
        let loss = DeviceLoss::new();
        instance.device_set_device_lost_closure(device, loss.reported_by_wgpu());
        Device {
            device: Arc::new(GpuObject::new(device, instance)),
            queue: Arc::new(GpuObject::new(queue, instance)),
            adapter,
            errors: Arc::new(ErrorSink::new()),
            loss,
            mappable_buffers: Arc::default(),
        }
    }

    pub fn device_id(&self) -> DeviceId {
        self.device.id()
    }

    /// The adapter the device was requested from, which names its GPU.
    pub fn adapter_id(&self) -> AdapterId {
        self.adapter.id()
    }

    /// The device's queue, for work a platform finishes on the program's behalf.
    pub fn queue_id(&self) -> QueueId {
        self.queue.id()
    }

    /// The instance the device is on.
    pub fn instance(&self) -> &Arc<Global> {
        self.device.instance()
    }

    /// Submits what the program wrote on the device's queue that no submission has carried yet,
    /// its `write-buffer`s and `write-texture`s, which wgpu-core keeps until the queue's next
    /// submission: an empty batch, as wgpu's `Queue::write_buffer_with` advises. Answers that
    /// submission, after every one the program made on the queue, for a platform that waits for
    /// everything the program submitted and wrote, such as the frame it drew; none where
    /// wgpu-core refused it, as on a lost device.
    ///
    /// It is GPU work, which an embedder that holds the program's submissions
    /// ([`SubmissionGate`](crate::SubmissionGate)) starts only where its gate allows, as
    /// `queue.submit` does.
    pub fn submit_pending_writes(&self) -> Option<wgpu_core::SubmissionIndex> {
        submit_pending_writes(&self.queue)
    }

    /// The device as an embedder keeps it: weakly, keeping nothing alive.
    pub fn downgrade(&self) -> WeakDevice {
        WeakDevice {
            device: Arc::downgrade(&self.device),
            queue: Arc::downgrade(&self.queue),
        }
    }

    /// Whether the device is lost or destroyed: what is made on it from then on is invalid, and
    /// what is drawn with it draws nothing.
    pub fn is_lost(&self) -> bool {
        self.loss.has_happened()
    }

    /// A program's handle on `texture`, a texture on this device that the embedder made and may
    /// keep a handle on too, described by `descriptor`, its errors reported to this device.
    pub fn texture(
        &self,
        texture: Arc<GpuObject<wgpu_core::id::TextureId>>,
        descriptor: crate::TextureDescriptor,
    ) -> Texture {
        Texture::new(texture, descriptor, Arc::clone(&self.errors))
    }

    /// Registers `buffer` as one `destroy` unmaps.
    pub(crate) fn add_mappable_buffer(&self, buffer: &Arc<GpuObject<BufferId>>) {
        self.mappable_buffers.add(buffer);
    }
}

/// A device as an embedder keeps it after the program opened it: weakly, so the embedder keeps
/// nothing alive. It answers the device while the program, or anything else, still holds it.
#[derive(Clone)]
pub struct WeakDevice {
    device: Weak<GpuObject<DeviceId>>,
    queue: Weak<GpuObject<QueueId>>,
}

impl WeakDevice {
    /// The device, held while the answer lives, for a wait on it; none once it was dropped.
    pub fn upgrade(&self) -> Option<Arc<GpuObject<DeviceId>>> {
        self.device.upgrade()
    }

    /// The device, held while the answer lives, once what the program wrote on its queue is
    /// submitted, with that submission, as [`Device::submit_pending_writes`] answers it; none once
    /// the device was dropped.
    pub fn submit_pending_writes(
        &self,
    ) -> Option<(Arc<GpuObject<DeviceId>>, Option<wgpu_core::SubmissionIndex>)> {
        let device = self.device.upgrade()?;
        let queue = self.queue.upgrade()?;
        Some((device, submit_pending_writes(&queue)))
    }
}

/// Submits an empty batch on `queue`, which carries what the program wrote on it.
fn submit_pending_writes(queue: &GpuObject<QueueId>) -> Option<wgpu_core::SubmissionIndex> {
    queue.instance().queue_submit(queue.id(), &[]).ok()
}

/// A device's buffers that can be mapped, which `destroy` unmaps.
#[derive(Default)]
struct MappableBuffers(Mutex<Vec<Weak<GpuObject<BufferId>>>>);

impl MappableBuffers {
    /// Adds `buffer`, and forgets the buffers the program has let go of.
    fn add(&self, buffer: &Arc<GpuObject<BufferId>>) {
        let mut buffers = self.0.lock().unwrap();
        buffers.retain(|buffer| buffer.strong_count() > 0);
        buffers.push(Arc::downgrade(buffer));
    }

    /// Unmaps every buffer, aborting a mapping still pending.
    fn unmap_all(&self) {
        for buffer in self.0.lock().unwrap().iter().filter_map(Weak::upgrade) {
            // A buffer not mapped refuses, and the device is about to be lost, which reports
            // nothing.
            let _ = buffer.instance().buffer_unmap(buffer.id());
        }
    }
}

/// Waits on a thread of its own until `device` has done what `kind` asks, which runs the
/// callbacks that work answers, a mapping's and submitted work's; nothing else polls a
/// program's device.
///
/// The wait holds the device until wgpu-core has polled it: a device the program drops
/// meanwhile, or that goes with the program's store, is still in wgpu-core's registry, which
/// panics at an id it no longer has.
pub(crate) async fn poll(
    device: Device,
    kind: wgpu_types::PollType<wgpu_core::SubmissionIndex>,
) -> Result<wgpu_types::PollStatus, wgpu_core::device::WaitIdleError> {
    spawn_blocking(move || device.instance().device_poll(device.device_id(), kind)).await
}

/// The wait for `submission` to be done, and no later one: what a mapping or a queue's submitted
/// work waits for, as wgpu-core answers it, 0 where it waits for nothing submitted.
pub(crate) fn until_done(
    submission: wgpu_core::SubmissionIndex,
) -> wgpu_types::PollType<wgpu_core::SubmissionIndex> {
    wgpu_types::PollType::Wait {
        submission_index: Some(submission),
        timeout: None,
    }
}

/// `poll`'s answer to the program: wgpu's poll error, or a trap for what wgpu answers with a
/// panic.
fn poll_answer(
    polled: Result<wgpu_types::PollStatus, wgpu_core::device::WaitIdleError>,
) -> wasmtime::Result<Result<PollStatus, PollError>> {
    match polled {
        Ok(status) => Ok(Ok(status.into())),
        Err(error) => match error.to_poll_error() {
            Some(error) => Ok(Err(error.into())),
            None => Err(wasmtime::format_err!("Device::poll: {error}")),
        },
    }
}

impl gpu::HostDevice for GpuCtx<'_> {
    fn features(&mut self, device: Resource<Device>) -> wasmtime::Result<Vec<Feature>> {
        let device = self.table.get(&device)?.device_id();
        Ok(features_of(self.instance.device_features(device)))
    }

    fn limits(
        &mut self,
        device: Resource<Device>,
    ) -> wasmtime::Result<Resource<wgpu_types::Limits>> {
        let device = self.table.get(&device)?.device_id();
        Ok(self.table.push(self.instance.device_limits(device))?)
    }

    fn adapter_info(&mut self, device: Resource<Device>) -> wasmtime::Result<AdapterInfo> {
        let adapter = self.table.get(&device)?.adapter_id();
        Ok(self.instance.adapter_get_info(adapter).into())
    }

    fn create_buffer(
        &mut self,
        device: Resource<Device>,
        descriptor: BufferDescriptor,
    ) -> wasmtime::Result<Resource<Buffer>> {
        let device = self.table.get(&device)?.clone();
        let buffer = Buffer::create(&device, descriptor.into());
        Ok(self.table.push(buffer)?)
    }

    fn create_texture(
        &mut self,
        device: Resource<Device>,
        descriptor: TextureDescriptor,
    ) -> wasmtime::Result<Resource<Texture>> {
        let device = self.table.get(&device)?;
        let texture = Texture::create(device, descriptor.into());
        Ok(self.table.push(texture)?)
    }

    fn create_external_texture(
        &mut self,
        device: Resource<Device>,
        descriptor: ExternalTextureDescriptor,
        planes: Vec<Resource<TextureView>>,
    ) -> wasmtime::Result<Resource<ExternalTexture>> {
        let descriptor = external_texture_descriptor(descriptor)?;
        let planes = planes
            .iter()
            .map(|plane| Ok(self.table.get(plane)?.id()))
            .collect::<wasmtime::Result<Vec<_>>>()?;
        let device = self.table.get(&device)?;
        let (texture, error) = self.instance.device_create_external_texture(
            device.device_id(),
            &descriptor,
            &planes,
            None,
        );
        device.errors.report(
            error,
            "Device::create_external_texture",
            descriptor.label.as_deref(),
        );
        Ok(self.table.push(GpuObject::new(texture, self.instance))?)
    }

    fn create_sampler(
        &mut self,
        device: Resource<Device>,
        descriptor: SamplerDescriptor,
    ) -> wasmtime::Result<Resource<Sampler>> {
        let descriptor = descriptor.into();
        let device = self.table.get(&device)?;
        let (sampler, error) =
            self.instance
                .device_create_sampler(device.device_id(), &descriptor, None);
        device
            .errors
            .report(error, "Device::create_sampler", descriptor.label.as_deref());
        Ok(self.table.push(GpuObject::new(sampler, self.instance))?)
    }

    fn create_bind_group_layout(
        &mut self,
        device: Resource<Device>,
        descriptor: BindGroupLayoutDescriptor,
    ) -> wasmtime::Result<Resource<BindGroupLayout>> {
        let descriptor = descriptor.into();
        let device = self.table.get(&device)?;
        let (layout, error) =
            self.instance
                .device_create_bind_group_layout(device.device_id(), &descriptor, None);
        device.errors.report(
            error,
            "Device::create_bind_group_layout",
            descriptor.label.as_deref(),
        );
        Ok(self.table.push(GpuObject::new(layout, self.instance))?)
    }

    fn create_bind_group(
        &mut self,
        device: Resource<Device>,
        descriptor: BindGroupDescriptor,
    ) -> wasmtime::Result<Resource<BindGroup>> {
        let bind_group = crate::binding::create_bind_group(self, &device, descriptor)?;
        Ok(self.table.push(bind_group)?)
    }

    fn create_pipeline_layout(
        &mut self,
        device: Resource<Device>,
        descriptor: PipelineLayoutDescriptor,
    ) -> wasmtime::Result<Resource<PipelineLayout>> {
        let layout = crate::binding::create_pipeline_layout(self, &device, descriptor)?;
        Ok(self.table.push(layout)?)
    }

    fn create_shader_module(
        &mut self,
        device: Resource<Device>,
        descriptor: ShaderModuleDescriptor,
    ) -> wasmtime::Result<Resource<ShaderModule>> {
        let device = self.table.get(&device)?;
        let module = ShaderModule::create(device, descriptor);
        Ok(self.table.push(module)?)
    }

    fn create_render_pipeline(
        &mut self,
        device: Resource<Device>,
        descriptor: RenderPipelineDescriptor,
    ) -> wasmtime::Result<Resource<RenderPipeline>> {
        let pipeline = crate::pipelines::create_render_pipeline(self, &device, descriptor)?;
        Ok(self.table.push(pipeline)?)
    }

    fn create_compute_pipeline(
        &mut self,
        device: Resource<Device>,
        descriptor: ComputePipelineDescriptor,
    ) -> wasmtime::Result<Resource<ComputePipeline>> {
        let pipeline = crate::pipelines::create_compute_pipeline(self, &device, descriptor)?;
        Ok(self.table.push(pipeline)?)
    }

    fn create_query_set(
        &mut self,
        device: Resource<Device>,
        descriptor: QuerySetDescriptor,
    ) -> wasmtime::Result<Resource<QuerySet>> {
        let descriptor = descriptor.into();
        let device = self.table.get(&device)?;
        let (query_set, error) =
            self.instance
                .device_create_query_set(device.device_id(), &descriptor, None);
        device.errors.report(
            error,
            "Device::create_query_set",
            descriptor.label.as_deref(),
        );
        Ok(self.table.push(GpuObject::new(query_set, self.instance))?)
    }

    fn create_command_encoder(
        &mut self,
        device: Resource<Device>,
        label_given: Option<String>,
    ) -> wasmtime::Result<Resource<CommandEncoder>> {
        let device = self.table.get(&device)?;
        let encoder = CommandEncoder::create(device, label(label_given));
        Ok(self.table.push(encoder)?)
    }

    fn create_render_bundle_encoder(
        &mut self,
        device: Resource<Device>,
        descriptor: RenderBundleEncoderDescriptor,
    ) -> wasmtime::Result<Resource<RenderBundleEncoder>> {
        let device = self.table.get(&device)?;
        let encoder = RenderBundleEncoder::create(device, descriptor.into());
        Ok(self.table.push(encoder)?)
    }

    fn push_error_scope(
        &mut self,
        device: Resource<Device>,
        filter: ErrorFilter,
    ) -> wasmtime::Result<()> {
        self.table.get(&device)?.errors.push_scope(filter);
        Ok(())
    }

    fn destroy(&mut self, device: Resource<Device>) -> wasmtime::Result<()> {
        let device = self.table.get(&device)?;
        device.mappable_buffers.unmap_all();
        self.instance.device_destroy(device.device_id());
        device.loss.lose(
            wgpu_types::DeviceLostReason::Destroyed,
            "the device was destroyed".to_owned(),
        );
        // wgpu-core frees what the device held at a poll that finds its queue empty, which this
        // one does unless work is still on the GPU.
        if let Err(error) = self
            .instance
            .device_poll(device.device_id(), wgpu_types::PollType::Poll)
        {
            tracing::warn!("polling a destroyed device: {error}");
        }
        Ok(())
    }

    fn drop(&mut self, device: Resource<Device>) -> wasmtime::Result<()> {
        self.table.delete(device)?;
        Ok(())
    }
}

impl<T: Send + 'static> gpu::HostDeviceWithStore<T> for HasGpuCtx {
    async fn pop_error_scope(
        accessor: &Accessor<T, Self>,
        device: Resource<Device>,
    ) -> wasmtime::Result<Option<Error>> {
        accessor.with(|mut access| {
            access
                .get()
                .table
                .get(&device)?
                .errors
                .pop_scope()
                .ok_or_else(|| wasmtime::format_err!("pop-error-scope with no error scope open"))
        })
    }

    fn uncaptured_errors(
        mut access: Access<T, Self>,
        device: Resource<Device>,
    ) -> wasmtime::Result<Option<StreamReader<Error>>> {
        let errors = access.get().table.get(&device)?.errors.take_uncaptured();
        errors
            .map(|errors| StreamReader::new(access, Uncaptured(errors)))
            .transpose()
    }

    fn lost(
        mut access: Access<T, Self>,
        device: Resource<Device>,
    ) -> wasmtime::Result<FutureReader<DeviceLostInfo>> {
        let info = access.get().table.get(&device)?.loss.info();
        FutureReader::new(access, Lost(Some(info)))
    }

    async fn poll(
        accessor: &Accessor<T, Self>,
        device: Resource<Device>,
        kind: PollType,
    ) -> wasmtime::Result<Result<PollStatus, PollError>> {
        let device = accessor.with(|mut access| -> wasmtime::Result<_> {
            Ok(access.get().table.get(&device)?.clone())
        })?;
        poll_answer(poll(device, kind.into()).await)
    }
}

/// `uncaptured-errors` as a component-model stream: each error no scope caught, one per read.
struct Uncaptured(futures::channel::mpsc::Receiver<Error>);

impl<D> StreamProducer<D> for Uncaptured {
    type Item = Error;
    type Buffer = Option<Error>;

    fn poll_produce<'a>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        _store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, Error, Option<Error>>,
        finish: bool,
    ) -> std::task::Poll<wasmtime::Result<StreamResult>> {
        use std::task::Poll;
        match self.get_mut().0.poll_next_unpin(cx) {
            Poll::Ready(Some(error)) => {
                destination.set_buffer(Some(error));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(None) => Poll::Ready(Ok(StreamResult::Dropped)),
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// `lost` as a component-model future: nothing until the device is lost, then its info. The
/// loss's info, until the device is dropped without being lost, when it never will be: the info
/// is let go of then, since a shared future answers once and panics when polled again.
struct Lost(Option<LostInfo>);

impl<D> wasmtime::component::FutureProducer<D> for Lost {
    type Item = DeviceLostInfo;

    fn poll_produce(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        _store: StoreContextMut<D>,
        finish: bool,
    ) -> std::task::Poll<wasmtime::Result<Option<DeviceLostInfo>>> {
        use std::task::Poll;
        let lost = self.get_mut();
        if let Some(info) = &mut lost.0 {
            match info.poll_unpin(cx) {
                Poll::Ready(Ok(info)) => return Poll::Ready(Ok(Some(info.into()))),
                Poll::Ready(Err(_)) => lost.0 = None,
                Poll::Pending => {}
            }
        }
        // The program stopped waiting, or waits on for a loss that may never come.
        if finish {
            Poll::Ready(Ok(None))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device on wgpu-core's noop backend, which needs no GPU.
    fn noop_device() -> Device {
        let instance = crate::noop::instance();
        let adapter = instance
            .request_adapter(
                &wgpu_types::RequestAdapterOptions::default(),
                wgpu_types::Backends::NOOP,
                None,
            )
            .expect("the noop backend's adapter");
        let (device, queue) = instance
            .adapter_request_device(
                adapter,
                &wgpu_types::DeviceDescriptor::default(),
                None,
                None,
            )
            .expect("a device on the noop adapter");
        let adapter = Arc::new(GpuObject::new(adapter, &instance));
        Device::opened(&instance, adapter, device, queue)
    }

    #[test]
    fn a_wait_holds_the_device_the_program_lets_go_of_meanwhile() {
        let device = noop_device();
        let wait = poll(device.clone(), wgpu_types::PollType::wait_indefinitely());
        drop(device);
        let polled = wasmtime_wasi::runtime::in_tokio(wait);
        assert!(polled.is_ok(), "{polled:?}");
    }

    /// The submission wgpu-core answers a mapping with is the one the wait is for: here none,
    /// since nothing was submitted, and the wait for it still runs the mapping's callback.
    #[test]
    fn a_wait_for_the_submission_a_mapping_answers_runs_its_callback() {
        let device = noop_device();
        let instance = Arc::clone(device.instance());
        let descriptor = wgpu_core::resource::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu_types::BufferUsages::MAP_READ | wgpu_types::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        };
        let buffer = Buffer::create(&device, descriptor);
        let (answer, mut mapped) = futures::channel::oneshot::channel();
        let operation = wgpu_core::resource::BufferMapOperation {
            host: wgpu_core::device::HostMap::Read,
            callback: Some(Box::new(move |result| {
                let _ = answer.send(result);
            })),
        };
        let submission = instance
            .buffer_map_async(buffer.id(), 0, None, operation)
            .expect("a buffer made to be read maps");
        assert_eq!(submission, 0, "nothing was submitted");
        let polled = wasmtime_wasi::runtime::in_tokio(poll(device, until_done(submission)));
        assert!(polled.is_ok(), "{polled:?}");
        assert!(matches!(mapped.try_recv(), Ok(Some(Ok(())))));
    }
}
