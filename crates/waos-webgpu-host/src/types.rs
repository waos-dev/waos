// Modified for WaOS by the waos Authors from the wasi-gfx runtime, as NOTICE says.
// Wrappers around `wgpu_*` types
// Every type here should have an explanation as to why we can't use the type directly.

use std::{
    collections::HashMap,
    fmt::{self, Debug},
    sync::{Arc, Mutex, Weak},
};

use wgpu_core::id;

use crate::buffer_mapping::{MappableBuffer, MappableBuffers};
use crate::device_loss::DeviceLoss;
use crate::gpu_object::GpuObject;
use crate::labels::ObjectLabel;
use crate::wasi::webgpu::webgpu;

pub struct WgslLanguageFeatures;
impl WgslLanguageFeatures {
    pub fn new() -> Self {
        Self
    }
    pub fn has(&self, feature: &str) -> bool {
        use wgpu_core::naga::front::wgsl::LanguageExtension;
        matches!(
            LanguageExtension::from_ident(feature),
            Some(LanguageExtension::Implemented(_))
        )
    }
}

// can't pass generics to `wasmtime::component::bindgen`
// TODO: these should be unit-structs instead of `types` so that the internals are private to the crate
pub type RecordGpuPipelineConstantValue = HashMap<String, webgpu::GpuPipelineConstantValue>;
pub type RecordOptionGpuSize64 = HashMap<String, Option<webgpu::GpuSize64>>;

/// A render pass as the program holds it: wgpu-core's pass, which knows whether it has ended and
/// stays after `end` so a later call on it is refused rather than reaching a pass that is gone, and
/// the device's error handler, where the refusal is reported.
pub struct RenderPassEncoder {
    pub(crate) pass: wgpu_core::command::RenderPass,
    pub(crate) error_handler: Arc<ErrorHandler>,
    pub(crate) label: ObjectLabel,
}

/// A compute pass as the program holds it, as [`RenderPassEncoder`] holds a render pass.
pub struct ComputePassEncoder {
    pub(crate) pass: wgpu_core::command::ComputePass,
    pub(crate) error_handler: Arc<ErrorHandler>,
    pub(crate) label: ObjectLabel,
}

/// A render bundle encoder as the program holds it: wgpu-core's encoder, which stays after
/// `finish`, and the device's error handler.
pub struct RenderBundleEncoder {
    pub(crate) encoder: wgpu_core::command::RenderBundleEncoder,
    pub(crate) error_handler: Arc<ErrorHandler>,
    pub(crate) label: ObjectLabel,
}

// The resources whose object is nothing but its id: the program's handle owns the object.
pub type RenderBundle = GpuObject<id::RenderBundleId>;
pub type CommandBuffer = GpuObject<id::CommandBufferId>;
pub type PipelineLayout = GpuObject<id::PipelineLayoutId>;
pub type BindGroupLayout = GpuObject<id::BindGroupLayoutId>;
pub type BindGroup = GpuObject<id::BindGroupId>;
pub type Sampler = GpuObject<id::SamplerId>;
pub type TextureView = GpuObject<id::TextureViewId>;

// size needed in `GpuBuffer.size`, `RenderPass.set_index_buffer`, `RenderPass.set_vertex_buffer`.
// usage needed in `GpuBuffer.usage`
// device needed to wait for a mapping on, and to report its errors to.
pub struct Buffer {
    pub(crate) object: Arc<MappableBuffer>,
    pub(crate) device: Device,
    pub(crate) size: u64,
    pub(crate) usage: wgpu_types::BufferUsages,
}
impl Buffer {
    pub(crate) fn id(&self) -> id::BufferId {
        self.object.id()
    }

    pub(crate) fn map_state(&self) -> webgpu::GpuBufferMapState {
        self.object.map_state()
    }

    pub(crate) fn set_map_state(&self, map_state: webgpu::GpuBufferMapState) {
        self.object.set_map_state(map_state);
    }
}

// Shared by the program's adapter handle and every device requested from it.
pub type Adapter = Arc<GpuObject<id::AdapterId>>;

// The device's error handler is needed so a submission's validation errors reach the
// device's error scopes, as WebGPU specifies, instead of the host.
#[derive(Clone)]
pub struct Queue {
    pub(crate) queue: Arc<GpuObject<id::QueueId>>,
    /// The device the queue is of, which is waited on for the queue's submitted work.
    pub(crate) device: Arc<GpuObject<id::DeviceId>>,
    pub(crate) error_handler: Arc<ErrorHandler>,
}
impl Queue {
    pub(crate) fn id(&self) -> id::QueueId {
        self.queue.id()
    }
}

/// A device as an embedder keeps it after the program opened it: weakly, so the embedder keeps
/// nothing alive. It answers the device while the program, or anything else, still holds it.
#[derive(Clone)]
pub struct WeakDevice {
    device: Weak<GpuObject<id::DeviceId>>,
    queue: Weak<GpuObject<id::QueueId>>,
}

impl WeakDevice {
    /// The device, held while the answer lives, for a wait on it; none once it was dropped.
    pub fn upgrade(&self) -> Option<Arc<GpuObject<id::DeviceId>>> {
        self.device.upgrade()
    }

    /// The device, held while the answer lives, once what the program wrote on its queue is
    /// submitted, with that submission, as [`Device::submit_pending_writes`] answers it, for a
    /// wait on everything the program submitted and wrote there so far; none once the device was
    /// dropped.
    pub fn submit_pending_writes(
        &self,
    ) -> Option<(
        Arc<GpuObject<id::DeviceId>>,
        Option<wgpu_core::SubmissionIndex>,
    )> {
        let device = self.device.upgrade()?;
        let queue = self.queue.upgrade()?;
        Some((device, submit_pending_writes(&queue)))
    }
}

/// Submits an empty batch on `queue`, which carries what the program wrote on it:
/// [`Device::submit_pending_writes`].
fn submit_pending_writes(queue: &GpuObject<id::QueueId>) -> Option<wgpu_core::SubmissionIndex> {
    queue.instance().queue_submit(queue.id(), &[]).ok()
}

/// A device with its queue, its adapter and its loss, as every handle on it shares them: the
/// program's own, a canvas context configured with it, and any a platform keeps for work it
/// finishes on the program's behalf. The last handle to go drops the device, its queue and its
/// adapter, so none is dropped while a platform still waits on it.
#[derive(Clone)]
pub struct Device {
    pub(crate) device: Arc<GpuObject<id::DeviceId>>,
    pub(crate) queue: Arc<GpuObject<id::QueueId>>,
    pub(crate) adapter: Adapter,
    pub(crate) error_handler: Arc<ErrorHandler>,
    pub(crate) loss: Arc<DeviceLoss>,
    pub(crate) mappable_buffers: Arc<MappableBuffers>,
}
impl Device {
    pub fn device_id(&self) -> id::DeviceId {
        self.device.id()
    }

    /// The adapter the device was requested from, which names its GPU.
    pub fn adapter_id(&self) -> id::AdapterId {
        self.adapter.id()
    }

    /// The device's queue, for work a platform finishes on the program's behalf.
    pub fn queue_id(&self) -> id::QueueId {
        self.queue.id()
    }

    /// Submits what the program wrote on the device's queue that no submission has carried yet,
    /// its `write-buffer`s and `write-texture`s, which wgpu-core keeps until the queue's next
    /// submission: an empty batch, as wgpu's `Queue::write_buffer_with` advises for this. Answers
    /// that submission, after every one the program made on the queue, for a platform that waits
    /// for everything the program submitted and wrote, such as the frame it drew; none where
    /// wgpu-core refused it, as on a lost device, where a wait for the last submission does.
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

    /// A program's handle on `texture`, a texture on this device that a canvas or the embedder
    /// made and may keep a handle on too, its errors reported to this device's error scopes.
    #[doc(hidden)]
    pub fn connect_texture(
        &self,
        texture: Arc<GpuObject<id::TextureId>>,
        attributes: TextureAttributes,
    ) -> Texture {
        Texture {
            texture,
            attributes,
            error_handler: Arc::clone(&self.error_handler),
        }
    }
}

pub struct CommandEncoder {
    pub(crate) command_encoder: GpuObject<id::CommandEncoderId>,
    pub(crate) error_handler: Arc<ErrorHandler>,
}
impl CommandEncoder {
    pub(crate) fn id(&self) -> id::CommandEncoderId {
        self.command_encoder.id()
    }
}

/// A program's handle on a texture. A texture it made is its alone; a canvas's current texture is
/// shared with the canvas, which keeps it until the frame is presented, as WebGPU's canvas keeps
/// its current texture.
pub struct Texture {
    pub(crate) texture: Arc<GpuObject<id::TextureId>>,
    pub(crate) attributes: TextureAttributes,
    pub(crate) error_handler: Arc<ErrorHandler>,
}

/// What a WebGPU texture answers of itself, as it was made: wgpu-core keeps no way to ask.
/// https://www.w3.org/TR/webgpu/#gputexture
#[derive(Clone, Copy)]
pub struct TextureAttributes {
    pub size: wgpu_types::Extent3d,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub dimension: webgpu::GpuTextureDimension,
    pub format: webgpu::GpuTextureFormat,
    pub usage: webgpu::GpuTextureUsage,
    pub texture_binding_view_dimension: Option<webgpu::GpuTextureViewDimension>,
}

impl TextureAttributes {
    /// A texture's attributes as `descriptor` describes it, WebGPU's defaults where it says
    /// nothing, and `size` as wgpu-core took it.
    /// https://www.w3.org/TR/webgpu/#dictdef-gputexturedescriptor
    pub(crate) fn of(
        descriptor: &webgpu::GpuTextureDescriptor,
        size: wgpu_types::Extent3d,
    ) -> Self {
        TextureAttributes {
            size,
            mip_level_count: descriptor.mip_level_count.unwrap_or(1),
            sample_count: descriptor.sample_count.unwrap_or(1),
            dimension: descriptor
                .dimension
                .unwrap_or(webgpu::GpuTextureDimension::D2),
            format: descriptor.format,
            usage: descriptor.usage,
            texture_binding_view_dimension: descriptor.texture_binding_view_dimension,
        }
    }
}

/// A query set as the program holds it: its object, and the type and count WebGPU's query set
/// answers, which wgpu-core keeps no way to ask.
pub struct QuerySet {
    pub(crate) query_set: GpuObject<id::QuerySetId>,
    pub(crate) type_: webgpu::GpuQueryType,
    pub(crate) count: u32,
}
impl QuerySet {
    pub(crate) fn id(&self) -> id::QuerySetId {
        self.query_set.id()
    }
}
impl Texture {
    pub(crate) fn id(&self) -> id::TextureId {
        self.texture.id()
    }
}

pub struct RenderPipeline {
    pub(crate) render_pipeline: GpuObject<id::RenderPipelineId>,
    pub(crate) error_handler: Arc<ErrorHandler>,
}
impl RenderPipeline {
    pub(crate) fn id(&self) -> id::RenderPipelineId {
        self.render_pipeline.id()
    }
}

pub struct ComputePipeline {
    pub(crate) compute_pipeline: GpuObject<id::ComputePipelineId>,
    pub(crate) error_handler: Arc<ErrorHandler>,
}
impl ComputePipeline {
    pub(crate) fn id(&self) -> id::ComputePipelineId {
        self.compute_pipeline.id()
    }
}

#[derive(Debug, Clone)]
pub struct GpuError {
    pub(crate) message: String,
    pub(crate) kind: webgpu::GpuErrorKind,
}

/// How many uncaptured errors one device writes to standard error before it stops: enough to
/// read the first ones by, not so many that a device failing every frame floods the output.
const UNCAPTURED_ERRORS_REPORTED: u32 = 100;

// Device level error handler. No errors are generated from a lost device, which reports its
// loss through `lost` instead: https://www.w3.org/TR/webgpu/#errors-and-debugging
#[derive(Debug)]
pub(crate) struct ErrorHandler {
    inner: Mutex<ErrorHandlerInner>,
    device_loss: Arc<DeviceLoss>,
}

#[derive(Debug)]
pub(crate) struct ErrorHandlerInner {
    scopes: Vec<ErrorScope>,
    /// Uncaptured errors written to standard error so far.
    uncaptured_errors_reported: u32,
    uncaptured_error_sender: async_broadcast::Sender<webgpu::GpuError>,
    // Keeping inactive receiver to keep channel open.
    // See https://docs.rs/async-broadcast/0.7.1/async_broadcast/struct.InactiveReceiver.html
    _uncaptured_error_receiver: async_broadcast::InactiveReceiver<webgpu::GpuError>,
}

impl ErrorHandler {
    pub(crate) fn new(device_loss: Arc<DeviceLoss>) -> Self {
        let (sender, receiver) = async_broadcast::broadcast(5);
        let receiver = receiver.deactivate();
        let inner = ErrorHandlerInner {
            scopes: Default::default(),
            uncaptured_errors_reported: 0,
            uncaptured_error_sender: sender,
            _uncaptured_error_receiver: receiver,
        };
        Self {
            inner: Mutex::new(inner),
            device_loss,
        }
    }
}

impl ErrorHandlerInner {
    /// Writes an error no scope captured to standard error, as a browser writes it to its
    /// console whether or not the page listens for it: the program's `uncapturederror` event
    /// is the program's to handle, and a program that does not must not fail silently.
    fn report_uncaptured(&mut self, error: &GpuError) {
        if self.uncaptured_errors_reported == UNCAPTURED_ERRORS_REPORTED {
            return;
        }
        self.uncaptured_errors_reported += 1;
        eprintln!(
            "webgpu: uncaptured {:?} error: {}",
            error.kind, error.message
        );
        if self.uncaptured_errors_reported == UNCAPTURED_ERRORS_REPORTED {
            eprintln!("webgpu: too many uncaptured errors; no more are reported for this device");
        }
    }
}

#[derive(Debug)]
struct ErrorScope {
    error: Option<webgpu::GpuError>,
    filter: webgpu::GpuErrorFilter,
}

impl ErrorHandler {
    pub fn push_scope(&self, filter: webgpu::GpuErrorFilter) {
        self.inner.lock().unwrap().scopes.push(ErrorScope {
            filter,
            error: None,
        });
    }

    pub fn pop_scope(&self) -> Result<Option<webgpu::GpuError>, webgpu::PopErrorScopeError> {
        // https://www.w3.org/TR/webgpu/#dom-gpudevice-poperrorscope
        if self.device_loss.has_happened() {
            return Ok(None);
        }
        let scopes = self.inner.lock().unwrap().scopes.pop();
        match scopes {
            Some(scope) => Ok(scope.error),
            None => {
                // From the spec:
                // > If any of the following requirements are unmet:
                // >  - this.[[errorScopeStack]].size must be > 0.
                // > Then issue the following steps on contentTimeline and return:
                // >  1. Reject promise with an OperationError.
                // https://www.w3.org/TR/webgpu/#dom-gpudevice-poperrorscope
                Err(webgpu::PopErrorScopeError {
                    kind: webgpu::PopErrorScopeErrorKind::OperationError,
                    message: "pop-error-scope on empty stack".to_string(),
                })
            }
        }
    }

    pub fn handle_possible_error<E: wgpu_types::error::WebGpuError + fmt::Display>(
        &self,
        error: Option<E>,
    ) {
        if let Some(error) = error {
            let kind = match error.webgpu_error_type() {
                wgpu_types::error::ErrorType::Validation => webgpu::GpuErrorKind::ValidationError,
                wgpu_types::error::ErrorType::OutOfMemory => webgpu::GpuErrorKind::OutOfMemoryError,
                wgpu_types::error::ErrorType::Internal => webgpu::GpuErrorKind::InternalError,
                wgpu_types::error::ErrorType::DeviceLost => return,
            };
            self.dispatch(GpuError {
                message: error.to_string(),
                kind,
            });
        }
    }

    /// Reports a validation error the host found itself, where wgpu-core has no error to give:
    /// a descriptor wgpu-core's types cannot hold, or an exception WebGPU throws that the WIT has
    /// no error result for, which is reported here rather than trapping the program.
    pub(crate) fn report_validation_error(&self, message: String) {
        self.dispatch(GpuError {
            message,
            kind: webgpu::GpuErrorKind::ValidationError,
        });
    }

    /// WebGPU's dispatch of an error: to the innermost scope that captures its kind, or else to
    /// the device's uncaptured errors.
    /// https://www.w3.org/TR/webgpu/#abstract-opdef-dispatch-error
    fn dispatch(&self, error: GpuError) {
        if self.device_loss.has_happened() {
            return;
        }
        let error_filter = error.kind.into();
        let mut inner = self.inner.lock().unwrap();
        match &mut inner
            .scopes
            .iter_mut()
            .rev()
            .find(|scope| scope.filter == error_filter)
        {
            Some(scope) => {
                // Only return one error per scope.
                // From the spec:
                // > 4. Let error be any one of the items in scope.[[errors]], or null if there are none.
                // >   For any two errors E1 and E2 in the list, if E2 was caused by E1, E2 should not be the one selected.
                // https://www.w3.org/TR/webgpu/#dom-gpudevice-poperrorscope
                // Here we're assuming that the first error is the one that caused the others, so only set the first error.
                if scope.error.is_none() {
                    scope.error = Some(error);
                }
            }
            None => {
                inner.report_uncaptured(&error);
                unwrap_unless_inactive_or_full(inner.uncaptured_error_sender.try_broadcast(error));
            }
        }
    }

    pub(crate) fn new_error_receiver(&self) -> async_broadcast::Receiver<webgpu::GpuError> {
        self.inner
            .lock()
            .unwrap()
            .uncaptured_error_sender
            .new_receiver()
    }
}

/// Ignores a broadcast that found its channel inactive or full: an uncaptured error nobody listens
/// for, or that a listener is too far behind to take. Folded in from the wasi-gfx runtime's
/// `shared` crate.
fn unwrap_unless_inactive_or_full<T>(res: Result<Option<T>, async_broadcast::TrySendError<T>>) {
    if let Err(e) = &res {
        if matches!(
            e,
            async_broadcast::TrySendError::Inactive(_) | async_broadcast::TrySendError::Full(_)
        ) {
            return;
        }
    }
    res.unwrap();
}

// For now, GpuErrorFilter and GpuErrorKind are effectively the same, just with different names.
impl From<webgpu::GpuErrorFilter> for webgpu::GpuErrorKind {
    fn from(filter: webgpu::GpuErrorFilter) -> Self {
        match filter {
            webgpu::GpuErrorFilter::Validation => webgpu::GpuErrorKind::ValidationError,
            webgpu::GpuErrorFilter::OutOfMemory => webgpu::GpuErrorKind::OutOfMemoryError,
            webgpu::GpuErrorFilter::Internal => webgpu::GpuErrorKind::InternalError,
        }
    }
}
impl From<webgpu::GpuErrorKind> for webgpu::GpuErrorFilter {
    fn from(kind: webgpu::GpuErrorKind) -> Self {
        match kind {
            webgpu::GpuErrorKind::ValidationError => webgpu::GpuErrorFilter::Validation,
            webgpu::GpuErrorKind::OutOfMemoryError => webgpu::GpuErrorFilter::OutOfMemory,
            webgpu::GpuErrorKind::InternalError => webgpu::GpuErrorFilter::Internal,
        }
    }
}
