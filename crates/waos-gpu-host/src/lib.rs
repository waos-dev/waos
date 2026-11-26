//! The host the engine serves `wasdk:gpu` with: wgpu's API over wgpu-core, each call of the
//! interface one call of wgpu-core's, as wgpu's own wgpu-core backend makes it
//! (`wgpu/src/backend/wgpu_core.rs`). Every object a program holds is a wgpu-core object in the
//! program's resource table ([`GpuObject`]); what wgpu-core refuses goes to the device's error
//! scopes or its uncaptured errors (`errors`), never into a call's result.
//!
//! The embedder has its say through [`GpuCtx`]: which adapter a program gets
//! ([`AdapterChooser`]), how its device is opened ([`DeviceOpener`]), when it may submit
//! ([`SubmissionGate`]), who is told of its devices ([`DeviceListener`]), and where a view's frames
//! come from ([`SurfaceTarget`]).
//!
//! The invariants: the wgpu crates are reached through [`reexports`], so the engine works at the
//! version this crate does; and a value crosses between the WIT and wgpu-types only through
//! `conversions`, whose matches are exhaustive, so a case wgpu adds fails to compile until it is
//! decided on.

mod adapters;
mod binding;
mod buffers;
mod commands;
mod conversions;
mod device_loss;
mod devices;
mod embedder;
mod errors;
mod gpu_object;
mod limits;
#[cfg(test)]
mod noop;
mod passes;
mod pipelines;
mod queues;
mod render_bundles;
mod shaders;
mod surfaces;
mod textures;

use std::sync::Arc;

use wasmtime::component::{HasData, ResourceTable};

pub use device_loss::{DeviceLoss, DeviceLostInfo};
pub use devices::{Device, WeakDevice};
pub use embedder::{AdapterChooser, DeviceListener, DeviceOpener, SubmissionGate};
pub use gpu_object::{GpuObject, GpuObjectId};
pub use surfaces::{Surface, SurfaceFrame, SurfaceTarget, WeakSurface};
pub use textures::{Texture, TextureDescriptor};

/// The wgpu crates this crate works at, so an embedder works at the same version.
pub mod reexports {
    pub use wgpu_core;
    pub use wgpu_types;
}

wasmtime::component::bindgen!({
    world: "waos:gpu-host/bindings",
    imports: {
        "wasdk:gpu/gpu.[method]device.lost": store | tracing | trappable,
        "wasdk:gpu/gpu.[method]device.uncaptured-errors": store | tracing | trappable,
        "wasdk:gpu/gpu.[method]buffer.map-async": store | tracing | trappable,
        "wasdk:gpu/gpu.[method]queue.on-submitted-work-done": store | tracing | trappable,
        // Waits for the embedder's submission gate before it submits; to the program it is
        // still a plain call.
        "wasdk:gpu/gpu.[method]queue.submit": async | store | tracing | trappable,
        default: tracing | trappable,
    },
    with: {
        "wasdk:gpu/types.limits": wgpu_types::Limits,
        "wasdk:gpu/gpu.adapter": adapters::Adapter,
        "wasdk:gpu/gpu.device": devices::Device,
        "wasdk:gpu/gpu.queue": queues::Queue,
        "wasdk:gpu/gpu.buffer": buffers::Buffer,
        "wasdk:gpu/gpu.texture": textures::Texture,
        "wasdk:gpu/gpu.texture-view": textures::TextureView,
        "wasdk:gpu/gpu.external-texture": textures::ExternalTexture,
        "wasdk:gpu/gpu.sampler": textures::Sampler,
        "wasdk:gpu/gpu.bind-group-layout": binding::BindGroupLayout,
        "wasdk:gpu/gpu.bind-group": binding::BindGroup,
        "wasdk:gpu/gpu.pipeline-layout": binding::PipelineLayout,
        "wasdk:gpu/gpu.shader-module": shaders::ShaderModule,
        "wasdk:gpu/gpu.render-pipeline": pipelines::RenderPipeline,
        "wasdk:gpu/gpu.compute-pipeline": pipelines::ComputePipeline,
        "wasdk:gpu/gpu.query-set": commands::QuerySet,
        "wasdk:gpu/gpu.command-encoder": commands::CommandEncoder,
        "wasdk:gpu/gpu.command-buffer": commands::CommandBuffer,
        "wasdk:gpu/gpu.compute-pass": passes::ComputePass,
        "wasdk:gpu/gpu.render-pass": passes::RenderPass,
        "wasdk:gpu/gpu.render-bundle-encoder": render_bundles::RenderBundleEncoder,
        "wasdk:gpu/gpu.render-bundle": render_bundles::RenderBundle,
        "wasdk:gpu/gpu.surface": surfaces::Surface,
    },
});

/// Adds `wasdk:gpu` to `linker`, served from each store's [`GpuCtxView`].
pub fn add_to_linker<T: GpuCtxView + 'static>(
    linker: &mut wasmtime::component::Linker<T>,
) -> wasmtime::Result<()> {
    wasdk::gpu::types::add_to_linker::<_, HasGpuCtx>(linker, T::gpu_ctx)?;
    wasdk::gpu::gpu::add_to_linker::<_, HasGpuCtx>(linker, T::gpu_ctx)?;
    Ok(())
}

/// A store whose program is served `wasdk:gpu`.
pub trait GpuCtxView: Send {
    fn gpu_ctx(&mut self) -> GpuCtx<'_>;
}

/// What `wasdk:gpu` is served from: the engine's instance, the program's table, and the
/// embedder's say in the calls the platform decides.
pub struct GpuCtx<'a> {
    pub instance: &'a Arc<wgpu_core::global::Global>,
    pub table: &'a mut ResourceTable,
    /// Which adapter `request-adapter` gives: the embedder's choice, or wgpu-core's
    /// `request_adapter` when there is none.
    pub adapters: Option<&'a dyn AdapterChooser>,
    /// How `request-device` opens a device once it has checked the limits asked: the
    /// embedder's way, or wgpu-core's `adapter_request_device` when there is none.
    pub device_opener: Option<&'a dyn DeviceOpener>,
    /// When the program may submit GPU work: the embedder's say, awaited before every
    /// `queue.submit`, or none to let every submission through at once.
    pub submissions: Option<&'a dyn SubmissionGate>,
    /// Who is told of each device the program opens, or none.
    pub devices: Option<&'a dyn DeviceListener>,
}

struct HasGpuCtx;

impl HasData for HasGpuCtx {
    type Data<'a> = GpuCtx<'a>;
}
