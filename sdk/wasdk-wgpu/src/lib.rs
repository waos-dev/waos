//! A [`wgpu`] backend over the engine's `wasi:webgpu`, for a program built as a component.
//!
//! wgpu's `custom` feature lets an external crate implement its dispatch traits. This crate
//! implements them over the [`wasdk::webgpu`] bindings, so anything written against the `wgpu`
//! API (valo, and the Waft UI framework through it) runs in a component unchanged.
//! `wasi:webgpu` mirrors the WebGPU specification, so the object model mirrors wgpu's own
//! browser backend: each wgpu object owns the wasi resource behind it and drops it with itself,
//! and descriptors borrow those resources from their owners for the duration of a call.
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
//!
//! // A video frame's hardware buffer is sampled through a texture imported of it on the device:
//! let video_texture = wasdk_wgpu::import_texture(&device, &video_frame.hardware_buffer());
//! ```
//!
//! # Drawing on a view
//!
//! A view is drawn on through wgpu's surface, the one way this crate draws on one, as a browser's
//! canvas is (`surface`): a view is the window `Instance::create_surface` takes, by the window
//! handle `wasdk` gives it, or any window over one that answers the view's handle as its own, so
//! a framework's renderer that makes its surface from a window's handles runs unchanged. The surface offers the engine's canvas format, which the
//! canvas is drawn in, `PresentMode::Fifo`, opaque and premultiplied alpha, and drawing into,
//! sampling and copying its textures; `configure` sets the view's canvas to the size configured,
//! which its textures are from then on whatever the view's own size, and configures a canvas
//! context of the view's against the device, its view formats passed through, of which the
//! engine serves the format's sRGB counterpart. `Queue::present` presents the view, with the layers it holds beneath it
//! (`view.set_layers`); a frame dropped without a present is not presented.
//!
//! # What the boundary cannot express
//!
//! `wasi:webgpu@0.3.0-rc.2` is the WebGPU API and nothing beyond it. Everything wgpu offers
//! past WebGPU panics at the call with a message naming the gap, or returns the error where the
//! wgpu signature has one. The complete list:
//!
//! - **Surfaces on anything but a view.** `Instance::create_surface` with a window handle that is
//!   not a view's, or a target that is not a window handle: wgpu gives a backend of its
//!   own no way to make the `CreateSurfaceError` it would answer. A surface's present modes but
//!   `Fifo` and the automatic ones, its alpha modes `PostMultiplied` and `Inherit`, and colour
//!   spaces a WebGPU canvas has no name for (`ExtendedSrgbLinear`, `Bt2100Pq`, `Bt2100Hlg`).
//! - **`as_hal` and pipeline caches.** There is no hal underneath; `create_pipeline_cache` and
//!   a `cache` in a pipeline descriptor.
//! - **Ray tracing.** BLAS/TLAS creation, compaction and builds,
//!   `BindingType::AccelerationStructure`, `BufferUsages::BLAS_INPUT | TLAS_INPUT`.
//! - **External textures.** `create_external_texture`, `BindingType::ExternalTexture`.
//! - **Passthrough shaders in a language other than WGSL.** A shader given as naga's module is
//!   written as WGSL on its way, and SPIR-V and GLSL are with this crate's `spirv` and `glsl`
//!   features, as wgpu's browser backend writes them; without the feature they stop the program.
//! - **Binding arrays.** `BindGroupLayoutEntry::count`, `BindingResource::*Array`.
//! - **Transient attachments.** `TextureUsages::TRANSIENT_ATTACHMENT` is a hint the host
//!   rejects, so it is stripped, as wgpu's browser backend strips it.
//! - **Browser-only entry points.** `Queue::copy_external_image_to_texture` and a mapped
//!   range's `as_uint8array`, which exist on wgpu's API only for the web.
//! - **Mesh shaders.** `create_mesh_pipeline`, `draw_mesh_tasks*`.
//! - **`multi_draw_*_count`.** Plain `multi_draw_indirect` is unrolled into draws, as on the web.
//! - **Timestamps inside encoders and passes** (`write_timestamp`): WebGPU writes them only at
//!   pass boundaries (`timestamp_writes`). **Pipeline statistics queries.**
//! - **`clear_texture`.**
//! - **Values without a WebGPU name:** texture formats `R64Uint`, `NV12`, `P010` and HDR ASTC;
//!   `Float64*` vertex formats; `TextureUsages::STORAGE_ATOMIC`; `StorageTextureAccess::Atomic`;
//!   `AddressMode::ClampToBorder` and `SamplerDescriptor::border_color`; `TextureAspect::Plane*`;
//!   `PolygonMode::Line | Point` and conservative rasterization; multiview; shader stages beyond
//!   vertex, fragment and compute.
//! - **Features.** A required feature without a WebGPU name (wgpu-native's) fails
//!   `request_device` with a [`wgpu::RequestDeviceError`]. wasi features without a wgpu bit
//!   (`core-features-and-limits`, `texture-formats-tier1/2`, `texture-component-swizzle`) are
//!   not reported.
//!
//! Hints with no wasi counterpart and no effect on results are accepted and dropped:
//! `DeviceDescriptor::{trace, memory_hints, experimental_features}`,
//! `RequestAdapterOptions::apply_limit_buckets`, `ShaderRuntimeChecks`,
//! `PipelineCompilationOptions::zero_initialize_workgroup_memory` (WebGPU always zeroes) and
//! `start/stop_graphics_debugger_capture`. `transition_resources` is a no-op because WebGPU
//! tracks resource state itself.
//!
//! # Where the semantics differ from wgpu-core
//!
//! - **Mapping copies.** The boundary shares no memory: `get_mapped_range` copies the range into
//!   the component (`get-mapped-range-get-with-copy`), and a mutable view is copied back when it
//!   is dropped (`get-mapped-range-set-with-copy`); drop every `BufferViewMut` before `unmap`.
//!   The first range of a buffer made mapped (`mapped_at_creation`) is not copied in: it is
//!   zeros, which the program makes itself, so `create_buffer_init` sends its data once.
//! - **A mapping's and submitted work's callbacks run at `Device::poll` with `Wait`, or as the
//!   program yields**, whichever comes first, each once: code written for native wgpu, which
//!   asks, polls and reads at once, and code written for the web, which awaits, both work. They
//!   run in the order asked, each once the GPU has done its work, so `poll` does not return
//!   before them. `poll` with `Poll` delivers none: nothing here learns whether the GPU is done
//!   without waiting. `set_device_lost_callback` and `on_uncaptured_error` are tasks on the
//!   program's executor and run while it awaits. `request_adapter`, `request_device`,
//!   `pop_error_scope` and `get_compilation_info` are wasi async calls awaited by the futures
//!   wgpu returns.
//! - **`PollType::Wait` waits for all of the device's work, with no limit:** its submission index
//!   and its timeout are not honoured. Submission indices count submissions per queue.
//! - **A frame on a lost device** is handed out as good: the canvas hands out an invalid texture
//!   it cannot tell from a good one, and the loss comes through the device's lost callback.
//! - **`AdapterInfo`** reports `Backend::BrowserWebGpu` (the WebGPU API, not a browser) and zero
//!   vendor/device ids: the wasi info is strings, which land in `name` and `driver_info`.
//! - **`LoadOp::DontCare`** clears, as on the web.
//! - **`Queue::write_buffer_with`** returns `None` for a write that fails validation
//!   (alignment, bounds, `COPY_DST`), as wgpu documents, instead of raising a validation error.
//! - **Validation the boundary reports becomes a panic.** Six calls answer with an error the
//!   wgpu signature has nowhere to put: `Queue::write_buffer`, `Buffer::unmap`,
//!   `Buffer::get_mapped_range`, writing a mapped range back, `Device::create_query_set` and
//!   `set_bind_group`. Each panics with the engine's message rather than reaching an error scope,
//!   where wgpu-core would raise a validation error.

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
mod gap;
mod instance;
mod pass;
mod pipeline;
mod query;
mod queue;
mod runtime;
mod sampler;
mod shader;
mod surface;
mod texture;

use wasdk::gpu_interop;
use wasdk::hardware_buffers::HardwareBuffer;
use wasdk::video_decoders::{self, VideoDecoders};
use wgpu::custom::InstanceInterface;

pub use device::WasdkDevice;
pub use instance::WasdkInstance;
pub use surface::{WasdkSurface, WasdkSurfaceOutputDetail};
pub use texture::WasdkTexture;

/// The wgpu instance over the engine's WebGPU. `wgpu::Instance::new` on this target has no
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
    let texture = gpu_interop::import_texture(WasdkDevice::of(device).gpu_device(), buffer);
    wgpu_texture(texture, "hardware buffer")
}

/// One plane of `buffer` imported as a texture on `device`, as a `wgpu::Texture`: the plane at
/// `index` of a video frame's `planes`, of that plane's size and format, which holds the buffer
/// and its frame until it is dropped. A decoder's picture is drawn by sampling each of its planes
/// and mixing them as the frame's `color` says.
pub fn import_plane(device: &wgpu::Device, buffer: &HardwareBuffer, index: u32) -> wgpu::Texture {
    let texture = gpu_interop::import_plane(WasdkDevice::of(device).gpu_device(), buffer, index);
    wgpu_texture(texture, "hardware buffer plane")
}

/// The system's video decoders whose frames are for `device`'s GPU
/// (`wasdk:media/video-decoders`); none where the system decodes no video for programs.
pub fn video_decoders(device: &wgpu::Device) -> Option<VideoDecoders> {
    video_decoders::get_video_decoders(WasdkDevice::of(device).gpu_device())
}

/// The engine's `texture` as wgpu's, described as the engine made it.
fn wgpu_texture(texture: wasdk::webgpu::GpuTexture, label: &'static str) -> wgpu::Texture {
    let descriptor = wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: texture.width(),
            height: texture.height(),
            depth_or_array_layers: texture.depth_or_array_layers(),
        },
        mip_level_count: texture.mip_level_count(),
        sample_count: texture.sample_count(),
        dimension: convert::texture_dimension_from_wasi(texture.dimension()),
        format: convert::texture_format_from_wasi(texture.format()),
        usage: convert::texture_usages_from_wasi(texture.usage()),
        view_formats: &[],
    };
    wgpu::Texture::from_custom(WasdkTexture::new(texture), &descriptor)
}
