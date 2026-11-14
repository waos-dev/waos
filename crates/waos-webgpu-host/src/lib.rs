// Modified for WaOS by the waos Authors from the wasi-gfx runtime, as NOTICE says.
#![recursion_limit = "256"]
// TODO: in this file:
// - Remove all calls to `Default::default()`. Instead, manually set them, and link to the spec stating what defaults should be used.
// - Implement all todos.
// - Remove all unwraps.
// - Remove clippy allows, and either fix the code, or add comments explaining why it's okay to leave it.
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::new_without_default)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use wasmtime::component::HasData;

// ToCore trait used for resources, records, and variants.
// Into trait used for enums and flags, since they never need table access.
mod enum_conversions;
mod flags_conversions;
mod to_core_conversions;

mod buffer_mapping;
mod canvas;
mod command_encoding;
mod compilation_info;
mod device_loss;
mod device_request;
mod gpu_object;
mod labels;
mod stream_pipe;
mod trait_impls;
mod types;
pub use canvas::{CanvasContext, CanvasSurface, SwapchainSurface};
pub use device_loss::DeviceLostInfo;
pub use device_request::DeviceOpener;
pub use gpu_object::{GpuObject, GpuObjectId};
pub use types::*;

/// Re-export of `wgpu_core` and `wgpu_types` so that runtime implementors don't need to keep track of what version of wgpu this crate is using.
pub mod reexports {
    pub use wgpu_core;
    pub use wgpu_types;
}

// https://searchfox.org/mozilla-central/source/dom/webgpu/Instance.h#68
#[cfg(target_os = "android")]
const PREFERRED_CANVAS_FORMAT: wasi::webgpu::webgpu::GpuTextureFormat =
    wasi::webgpu::webgpu::GpuTextureFormat::Rgba8unorm;
#[cfg(not(target_os = "android"))]
const PREFERRED_CANVAS_FORMAT: wasi::webgpu::webgpu::GpuTextureFormat =
    wasi::webgpu::webgpu::GpuTextureFormat::Bgra8unorm;

#[cfg(all(
    not(target_os = "linux"),
    not(target_os = "android"),
    not(target_os = "windows"),
    not(target_os = "macos"),
    not(target_os = "ios"),
))]
pub(crate) type Backend = wgpu_core::api::Gl;

wasmtime::component::bindgen!({
    world: "wasi:webgpu/imports",
    imports: {
        "wasi:webgpu/webgpu.[method]gpu-device.lost": store | tracing | trappable,
        "wasi:webgpu/webgpu.[method]gpu-device.on-uncaptured-error": store | tracing | trappable,
        // Waits for the embedder's submission gate before it submits; to the program it is
        // still a plain call.
        "wasi:webgpu/webgpu.[method]gpu-queue.submit": async | store | tracing | trappable,
        default: tracing | trappable,
    },
    with: {
        "wasi:webgpu/webgpu.gpu-adapter": types::Adapter,
        "wasi:webgpu/webgpu.gpu-device": types::Device,
        "wasi:webgpu/webgpu.gpu-queue": types::Queue,
        "wasi:webgpu/webgpu.gpu-command-encoder": types::CommandEncoder,
        "wasi:webgpu/webgpu.gpu-render-pass-encoder": types::RenderPassEncoder,
        "wasi:webgpu/webgpu.gpu-compute-pass-encoder": types::ComputePassEncoder,
        "wasi:webgpu/webgpu.gpu-shader-module": compilation_info::ShaderModule,
        "wasi:webgpu/webgpu.gpu-compilation-info": compilation_info::CompilationInfo,
        "wasi:webgpu/webgpu.gpu-compilation-message": compilation_info::CompilationMessage,
        "wasi:webgpu/webgpu.gpu-render-pipeline": types::RenderPipeline,
        "wasi:webgpu/webgpu.gpu-render-bundle-encoder": types::RenderBundleEncoder,
        "wasi:webgpu/webgpu.gpu-render-bundle": types::RenderBundle,
        "wasi:webgpu/webgpu.gpu-command-buffer": types::CommandBuffer,
        "wasi:webgpu/webgpu.gpu-buffer": types::Buffer,
        "wasi:webgpu/webgpu.gpu-pipeline-layout": types::PipelineLayout,
        "wasi:webgpu/webgpu.gpu-bind-group-layout": types::BindGroupLayout,
        "wasi:webgpu/webgpu.gpu-sampler": types::Sampler,
        "wasi:webgpu/webgpu.gpu-supported-features": wgpu_types::Features,
        "wasi:webgpu/webgpu.gpu-texture": types::Texture,
        "wasi:webgpu/webgpu.gpu-canvas-context": canvas::CanvasContext,
        "wasi:webgpu/webgpu.gpu-compute-pipeline": types::ComputePipeline,
        "wasi:webgpu/webgpu.gpu-bind-group": types::BindGroup,
        "wasi:webgpu/webgpu.gpu-texture-view": types::TextureView,
        "wasi:webgpu/webgpu.gpu-adapter-info": wgpu_types::AdapterInfo,
        "wasi:webgpu/webgpu.gpu-query-set": types::QuerySet,
        "wasi:webgpu/webgpu.gpu-supported-limits": wgpu_types::Limits,
        "wasi:webgpu/webgpu.gpu-error": types::GpuError,
        "wasi:webgpu/webgpu.gpu-device-lost-info": device_loss::DeviceLostInfo,
        "wasi:webgpu/webgpu.wgsl-language-features": types::WgslLanguageFeatures,
        "wasi:webgpu/webgpu.record-gpu-pipeline-constant-value": types::RecordGpuPipelineConstantValue,
        "wasi:webgpu/webgpu.record-option-gpu-size64": types::RecordOptionGpuSize64,
    },
});

// linker connection
pub fn add_to_linker<T>(l: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
where
    T: WasiWebGpuCtxView,
{
    wasi::webgpu::webgpu::add_to_linker::<_, HasWasiWebGpuCtx>(l, T::webgpu_ctx)?;
    Ok(())
}

/// returns a struct of references.
/// Returning all references in a struct allows us to use different mutable references at the same time.
pub trait WasiWebGpuCtxView: Send {
    /// Return a [WasiWebGpu] from mutable reference to self.
    fn webgpu_ctx(&mut self) -> WasiWebGpuCtx<'_>;
}

pub struct WasiWebGpuCtx<'a> {
    // wrapped in arc to allow cloning for async. might be able to remove
    pub instance: &'a Arc<wgpu_core::global::Global>,
    pub table: &'a mut wasmtime_wasi::ResourceTable,
    /// How `request-device` opens a device once it has checked what the program asks: the
    /// embedder's way, or wgpu-core's `adapter_request_device` when there is none.
    pub device_opener: Option<&'a dyn DeviceOpener>,
    /// When the program may submit GPU work: the embedder's say, awaited before every
    /// `queue.submit`, or none to let every submission through at once.
    pub submissions: Option<&'a dyn SubmissionGate>,
    /// Who is told of each device the program opens, or none.
    pub devices: Option<&'a dyn DeviceListener>,
}

/// What an embedder is told of a program's devices: each one `request-device` opens, as the
/// program is handed it, so the embedder can wait for the GPU work on every device the program
/// holds without keeping any alive ([`Device::downgrade`]).
pub trait DeviceListener: Send + Sync {
    /// The program opened `device`.
    fn opened(&self, device: &Device);
}

/// When a program may submit GPU work. A platform that refuses GPU work from an app in the
/// background has its embedder hold a program's submissions while the program is held there;
/// wgpu-core submits GPU work at `queue.submit` alone, a queue write waiting in its pending
/// commands until the next, so that is the one call that asks. Passing the gate and submitting
/// are one step: the submission runs inside [`while_allowed`](Self::while_allowed), so a hold
/// that comes between the wait and the submission refuses it, and no submission starts once a
/// hold has taken.
pub trait SubmissionGate: Send + Sync {
    /// Resolves once the program may submit; at once when it may now.
    fn until_allowed(&self) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    /// Runs `submit` unless the program may not submit now, keeping that from changing while
    /// it runs; answers whether it ran. A caller answered no waits on `until_allowed` and asks
    /// again. `submit` is quick and takes nothing of the embedder's.
    fn while_allowed(&self, submit: &mut dyn FnMut()) -> bool;
}

struct HasWasiWebGpuCtx;

impl HasData for HasWasiWebGpuCtx {
    type Data<'a> = WasiWebGpuCtx<'a>;
}
