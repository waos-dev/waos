//! A [`wgpu`] backend over the engine's `wasdk:gpu`, for a program built as a component.
//!
//! wgpu's `custom` feature lets an external crate implement its dispatch traits. This crate
//! implements them over the [`wasdk::gpu`] bindings, so anything written against the `wgpu` API
//! (valo, and the Waft UI framework through it) runs in a component unchanged. `wasdk:gpu` is
//! wgpu's model as WIT, so each dispatch call is one call of the WIT, which the engine serves with
//! one call of wgpu-core: each wgpu object owns the engine's resource behind it and drops it with
//! itself, descriptors borrow those resources for the duration of a call, and values convert case
//! for case (`convert`). What wgpu-core refuses goes to the device's error scopes, or else to its
//! uncaptured-error handler, as with wgpu's own backend.
//!
//! ```ignore
//! let instance = wasdk_wgpu::instance();
//! let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await?;
//! let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await?;
//!
//! // A view is the window a surface is made for, shared with it as any window is:
//! let view = std::sync::Arc::new(view);
//! let surface = instance.create_surface(view.clone())?;
//! let metrics = view.metrics();
//! let config = surface.get_default_config(&adapter, metrics.width, metrics.height).unwrap();
//! surface.configure(&device, &config);
//!
//! // Each frame:
//! let wgpu::CurrentSurfaceTexture::Success(frame) = surface.get_current_texture() else { .. };
//! // ...draw into `frame.texture` and submit...
//! view.set_layers(&layers); // a program with composition layers
//! queue.present(frame); // presents the view
//!
//! // What makes GPU memory for the program makes it for a device, the GPU's it is on:
//! let decoders = wasdk_wgpu::video_decoders(&device);
//! let allocator = wasdk_wgpu::hardware_buffer_allocator(&device);
//!
//! // A hardware buffer is drawn into and sampled through a texture imported of it on the device,
//! // and a video frame's planes each through a texture and the aspect that is the plane:
//! let texture = wasdk_wgpu::import_texture(&device, &buffer);
//! let planes = wasdk_wgpu::import_planes(&device, &video_frame.hardware_buffer());
//! ```
//!
//! # Drawing on a view
//!
//! A view is drawn on through wgpu's surface, the one way this crate draws on one (`surface`): a
//! view is the window `Instance::create_surface` takes, by the window handle `wasdk` gives it, or
//! any window over one that answers the view's handle as its own, so a framework's renderer that
//! makes its surface from a window's handles runs unchanged. The surface is the view's own
//! (`view.surface()`): its capabilities say what the engine serves, `configure` sets the frames'
//! size, whatever the view's own, and `get_current_texture` answers `Occluded` while the view is
//! not on screen and `Validation` while the frame taken last is still held. `Queue::present`
//! presents the view, with the layers it holds beneath it (`view.set_layers`); a frame dropped
//! without a present is discarded, as with wgpu's own backends.
//!
//! # What `wasdk:gpu` leaves out
//!
//! Only wgpu's safe API reaches the engine. Each of these panics at the call, naming it:
//!
//! - **Surfaces on anything but a view.** `Instance::create_surface` with a window handle that is
//!   not a view's, or a target that is not a window handle: wgpu gives a backend of its own no
//!   way to make the `CreateSurfaceError` it would answer.
//! - **The experimental features**, which wgpu makes a program accept undefined behaviour to turn
//!   on: ray tracing (acceleration structures, their bindings, `BufferUsages::BLAS_INPUT |
//!   TLAS_INPUT` and the ray tracing stages), mesh shaders (`create_mesh_pipeline`,
//!   `draw_mesh_tasks*`, the task and mesh stages) and cooperative matrices. A required feature
//!   the WIT has no case for fails `request_device` with a [`wgpu::RequestDeviceError`]; the
//!   limits of the experimental features are not asked for and stay at wgpu's defaults.
//! - **What skips validation:** passthrough shaders (`create_shader_module_passthrough`) and
//!   pipeline caches (`create_pipeline_cache`, a pipeline descriptor's `cache`), which the engine
//!   keeps for every program alike.
//! - **Browser-only entry points:** `Queue::copy_external_image_to_texture` and a mapped range's
//!   `as_uint8array`.
//! - **SPIR-V and GLSL sources without this crate's `spirv` and `glsl` features**, which name
//!   wgpu's variants for them; with them, the engine's naga reads both.
//!
//! These are accepted, and do what the engine does instead: `ShaderRuntimeChecks` (naga's bounds
//! checks always run), `PipelineCompilationOptions::zero_initialize_workgroup_memory` (the engine
//! always zeroes), `LoadOp::DontCare` (it clears, as wgpu's backend for the web does), and the
//! engine's own hints and tools, `DeviceDescriptor::trace`,
//! `RequestAdapterOptions::apply_limit_buckets`, `start/stop_graphics_debugger_capture`,
//! `get_internal_counters` (empty) and `generate_allocator_report` (none). `transition_resources`
//! does nothing, as wgpu-core tracks resource state itself. A shader given as naga's module
//! (`ShaderSource::Naga`), which has no stable form to cross the boundary in, is written as WGSL
//! here first.
//!
//! # Where the semantics differ from wgpu-core
//!
//! - **Mapping copies.** The boundary shares no memory: `get_mapped_range` copies the range in,
//!   and a mutable view is copied back when it is dropped; drop every `BufferViewMut` before
//!   `unmap`. The first range of a buffer made mapped (`mapped_at_creation`) is not copied in: it
//!   is zeros, which the program makes itself, so `create_buffer_init` sends its data once. A
//!   range that is not mapped traps the program, where wgpu-core answers a
//!   [`wgpu::MapRangeError`], which no backend outside wgpu can make.
//! - **A mapping's and submitted work's callbacks run at `Device::poll`, or as the program
//!   yields**, whichever comes first, each once, in the order asked. The request reaches the
//!   engine at the call, as with wgpu's own backend, so an `unmap` aborts a mapping not yet
//!   answered, running its callback at once, and submitted work is told done once the work
//!   submitted before the ask is. `poll`, with `Wait` or `Poll`, runs every callback whose work is
//!   done when the engine's poll returns: code written for native wgpu, which asks, polls and
//!   reads at once, and code written for the web, which awaits, both work.
//! - **Uncaptured errors and device loss arrive as the program awaits.** The device's uncaptured
//!   errors are read by a task started as the device opens, which hands each to the handler set
//!   with `on_uncaptured_error`, or else panics, as wgpu's default handler does;
//!   `set_device_lost_callback` starts a task too. A device opened inside a `wasdk::block_on`
//!   keeps that wait from returning, since the wait waits for the task: open it from async code.
//! - **`Queue::write_buffer_with` never answers `None`**: the engine validates the write when the
//!   view is dropped, reporting what fails to the error scopes.

/// Recovers this backend's object from the wgpu API type or dispatch wrapper carrying it; an
/// object from another wgpu backend is a programming error.
macro_rules! downcast {
    ($ty:ident: api $api:ty) => {
        impl $ty {
            pub(crate) fn of(api: &$api) -> &Self {
                api.as_custom::<Self>().expect(concat!(
                    "wasdk-wgpu: a ",
                    stringify!($api),
                    " from another wgpu backend"
                ))
            }
        }
    };
    ($ty:ident: dispatch $dispatch:ty) => {
        impl $ty {
            pub(crate) fn of_dispatch(dispatch: &$dispatch) -> &Self {
                dispatch.as_custom::<Self>().expect(concat!(
                    "wasdk-wgpu: a ",
                    stringify!($dispatch),
                    " from another wgpu backend"
                ))
            }
        }
    };
    ($ty:ident: api $api:ty, dispatch $dispatch:ty) => {
        downcast!($ty: api $api);
        downcast!($ty: dispatch $dispatch);
    };
}

mod adapter;
mod binding;
mod buffer;
mod command;
pub mod convert;
mod device;
mod device_lost;
mod instance;
mod left_out;
mod pass;
mod pipeline;
mod query;
mod queue;
mod runtime;
mod sampler;
mod shader;
mod surface;
mod texture;
mod uncaptured_errors;

use wasdk::gpu_interop;
use wasdk::hardware_buffers::{self, HardwareBuffer, HardwareBufferAllocator};
use wasdk::video_decoders::{self, VideoDecoders};
use wgpu::custom::InstanceInterface;

pub use device::WasdkDevice;
pub use instance::WasdkInstance;
pub use surface::{WasdkSurface, WasdkSurfaceOutputDetail};
pub use texture::WasdkTexture;

/// The wgpu instance over the engine's `wasdk:gpu`. `wgpu::Instance::new` on this target has no
/// backend; this is the one to use.
pub fn instance() -> wgpu::Instance {
    let descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    wgpu::Instance::from_custom(<WasdkInstance as InstanceInterface>::new(descriptor))
}

/// `buffer` imported as a texture on `device`, over its memory, as a `wgpu::Texture`
/// (`wasdk:platform/gpu-interop`): a buffer the program allocated, an image the hardware codecs
/// decoded or a video frame's, which the texture holds, with its frame, until it is dropped. Its
/// size and format are the buffer's, and its usages follow from the buffer's usage.
pub fn import_texture(device: &wgpu::Device, buffer: &HardwareBuffer) -> wgpu::Texture {
    wgpu_texture(gpu_interop::import_texture(
        WasdkDevice::of(device).wit_device(),
        buffer,
    ))
}

/// One plane of a hardware buffer as the GPU reads it: a texture, and the aspect of it that is
/// the plane, which a view of the plane takes (`wasdk:platform/gpu-interop`'s `texture-plane`).
#[derive(Debug)]
pub struct TexturePlane {
    pub texture: wgpu::Texture,
    pub aspect: wgpu::TextureAspect,
}

impl TexturePlane {
    /// A view of the plane alone, to bind or to pass to `Device::create_external_texture`.
    pub fn create_view(&self) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            aspect: self.aspect,
            ..Default::default()
        })
    }
}

/// Each plane of `buffer` on `device`, in the order of a video frame's `planes`, each holding the
/// buffer and its frame until it is dropped. Where the device has the buffer's two-plane format
/// (D3D12, Vulkan), every plane is one `NV12` or `P010` texture with the plane's aspect;
/// elsewhere (Metal), each is a texture of its own with `All`. A decoder's picture is drawn by
/// sampling each plane's view and mixing them as the frame's `color` says, or through an external
/// texture made of the views.
pub fn import_planes(device: &wgpu::Device, buffer: &HardwareBuffer) -> Vec<TexturePlane> {
    let planes = gpu_interop::import_planes(WasdkDevice::of(device).wit_device(), buffer);
    (planes.into_iter())
        .map(|plane| TexturePlane {
            texture: wgpu_texture(plane.texture),
            aspect: convert::texture_aspect_from_wit(plane.aspect),
        })
        .collect()
}

/// The system's video decoders whose frames are for `device`'s GPU
/// (`wasdk:media/video-decoders`); none where the system decodes no video for programs.
pub fn video_decoders(device: &wgpu::Device) -> Option<VideoDecoders> {
    video_decoders::get_video_decoders(WasdkDevice::of(device).wit_device())
}

/// The allocator of hardware buffers drawn on `device` and shown
/// (`wasdk:platform/hardware-buffers`); none where the system allocates no hardware buffers.
pub fn hardware_buffer_allocator(device: &wgpu::Device) -> Option<HardwareBufferAllocator> {
    hardware_buffers::get_hardware_buffer_allocator(WasdkDevice::of(device).wit_device())
}

/// The engine's `texture` as wgpu's, described as the engine made it.
fn wgpu_texture(texture: wasdk::gpu::Texture) -> wgpu::Texture {
    let made = texture.descriptor();
    let descriptor = wgpu::TextureDescriptor {
        label: None,
        size: convert::extent_from_wit(made.size),
        mip_level_count: made.mip_level_count,
        sample_count: made.sample_count,
        dimension: convert::texture_dimension_from_wit(made.dimension),
        format: convert::texture_format_from_wit(made.format),
        usage: convert::texture_usages_from_wit(made.usage),
        view_formats: &[],
    };
    wgpu::Texture::from_custom(texture::WasdkTexture::new(texture), &descriptor)
}
