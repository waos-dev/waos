//! The device: every object it creates, its error scopes, and what happens to it.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use futures::StreamExt;
use wasdk::webgpu::GpuDevice;
use wgpu::custom::{
    BoxDeviceLostCallback, DeviceInterface, DispatchBindGroup, DispatchBindGroupLayout,
    DispatchBlas, DispatchBuffer, DispatchCommandEncoder, DispatchComputePipeline, DispatchDevice,
    DispatchExternalTexture, DispatchPipelineCache, DispatchPipelineLayout, DispatchQuerySet,
    DispatchRenderBundleEncoder, DispatchRenderPipeline, DispatchSampler, DispatchShaderModule,
    DispatchTexture, DispatchTlas, PopErrorScopeFuture,
};
use wgpu::{AdapterInfo, Features, Limits};

use crate::gap::wasi_gap;
use crate::runtime::OwedCallbacks;
use crate::{
    binding, buffer, command, convert, pass, pipeline, query, runtime, sampler, shader, texture,
};

/// The device behind a [`wgpu::Device`] from this backend: `device.as_custom::<WasdkDevice>()`.
#[derive(Debug)]
pub struct WasdkDevice {
    device: Arc<GpuDevice>,
    /// The callbacks it owes the program: its buffers' mappings and its queue's submitted work.
    owed: OwedCallbacks,
    /// How many error scopes are open, so a pop out of order is caught here.
    open_error_scopes: AtomicU32,
}

downcast!(WasdkDevice: api wgpu::Device, dispatch DispatchDevice);

impl WasdkDevice {
    pub(crate) fn new(device: GpuDevice, owed: OwedCallbacks) -> Self {
        WasdkDevice {
            device: Arc::new(device),
            owed,
            open_error_scopes: AtomicU32::new(0),
        }
    }

    /// The wasi device, which a view's canvas context is configured against and a hardware
    /// buffer is imported on as a texture.
    pub fn gpu_device(&self) -> &GpuDevice {
        &self.device
    }
}

// `create_shader_module_passthrough`, `create_pipeline_cache` and the debugger-capture pair
// are `unsafe` in the trait; nothing they do here is.
#[allow(unsafe_code)]
impl DeviceInterface for WasdkDevice {
    fn features(&self) -> Features {
        convert::features_from_wasi(&self.device.features())
    }

    fn limits(&self) -> Limits {
        convert::limits_from_wasi(&self.device.limits())
    }

    fn adapter_info(&self) -> AdapterInfo {
        convert::adapter_info_from_wasi(&self.device.adapter_info())
    }

    fn create_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
        _runtime_checks: wgpu::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        shader::create_shader_module(&self.device, &desc)
    }

    unsafe fn create_shader_module_passthrough(
        &self,
        desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        shader::create_shader_module_passthrough(&self.device, desc)
    }

    fn create_bind_group_layout(
        &self,
        desc: &wgpu::BindGroupLayoutDescriptor<'_>,
    ) -> DispatchBindGroupLayout {
        binding::create_bind_group_layout(&self.device, desc)
    }

    fn create_bind_group(&self, desc: &wgpu::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        binding::create_bind_group(&self.device, desc)
    }

    fn create_pipeline_layout(
        &self,
        desc: &wgpu::PipelineLayoutDescriptor<'_>,
    ) -> DispatchPipelineLayout {
        pipeline::create_pipeline_layout(&self.device, desc)
    }

    fn create_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        pipeline::create_render_pipeline(&self.device, desc)
    }

    fn create_mesh_pipeline(
        &self,
        _desc: &wgpu::MeshPipelineDescriptor<'_>,
    ) -> DispatchRenderPipeline {
        wasi_gap("mesh shaders (Device::create_mesh_pipeline)")
    }

    fn create_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> DispatchComputePipeline {
        pipeline::create_compute_pipeline(&self.device, desc)
    }

    unsafe fn create_pipeline_cache(
        &self,
        _desc: &wgpu::PipelineCacheDescriptor<'_>,
    ) -> DispatchPipelineCache {
        wasi_gap("pipeline caches (Device::create_pipeline_cache)")
    }

    fn create_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> DispatchBuffer {
        buffer::create_buffer(&self.device, &self.owed, desc)
    }

    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        texture::create_texture(&self.device, desc)
    }

    fn create_external_texture(
        &self,
        _desc: &wgpu::ExternalTextureDescriptor<'_>,
        _planes: &[&wgpu::TextureView],
    ) -> DispatchExternalTexture {
        wasi_gap("external textures (Device::create_external_texture)")
    }

    fn create_blas(
        &self,
        _desc: &wgpu::CreateBlasDescriptor<'_>,
        _sizes: wgpu::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        wasi_gap("ray tracing (Device::create_blas)")
    }

    fn create_tlas(&self, _desc: &wgpu::CreateTlasDescriptor<'_>) -> DispatchTlas {
        wasi_gap("ray tracing (Device::create_tlas)")
    }

    fn create_sampler(&self, desc: &wgpu::SamplerDescriptor<'_>) -> DispatchSampler {
        sampler::create_sampler(&self.device, desc)
    }

    fn create_query_set(&self, desc: &wgpu::QuerySetDescriptor<'_>) -> DispatchQuerySet {
        query::create_query_set(&self.device, desc)
    }

    fn create_command_encoder(
        &self,
        desc: &wgpu::CommandEncoderDescriptor<'_>,
    ) -> DispatchCommandEncoder {
        command::create_command_encoder(&self.device, desc)
    }

    fn create_render_bundle_encoder(
        &self,
        desc: &wgpu::RenderBundleEncoderDescriptor<'_>,
    ) -> DispatchRenderBundleEncoder {
        pass::create_render_bundle_encoder(&self.device, desc)
    }

    fn set_device_lost_callback(&self, callback: BoxDeviceLostCallback) {
        let lost = self.device.lost();
        runtime::spawn(async move {
            let info = lost.await;
            callback(
                convert::device_lost_reason_from_wasi(info.reason()),
                info.message(),
            );
        });
    }

    fn on_uncaptured_error(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {
        let mut errors = self.device.on_uncaptured_error().into_stream();
        runtime::spawn(async move {
            while let Some(error) = errors.next().await {
                handler(convert::error_from_wasi(&error));
            }
        });
    }

    fn push_error_scope(&self, filter: wgpu::ErrorFilter) -> u32 {
        let index = self.open_error_scopes.fetch_add(1, Ordering::Relaxed);
        self.device
            .push_error_scope(convert::error_filter_to_wasi(filter));
        index
    }

    fn pop_error_scope(&self, index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        let open = self.open_error_scopes.load(Ordering::Relaxed);
        assert!(
            open == index + 1,
            "wasdk-wgpu: error scope {index} popped with {open} open; scopes pop in reverse order"
        );
        self.open_error_scopes.store(index, Ordering::Relaxed);
        let device = Arc::clone(&self.device);
        Box::pin(async move {
            // The only wasi error is "no scope to pop", which the count above rules out.
            let error = device.pop_error_scope().await.ok().flatten()?;
            Some(convert::error_from_wasi(&error))
        })
    }

    unsafe fn start_graphics_debugger_capture(&self) {
        // WebGPU has no capture API.
    }

    unsafe fn stop_graphics_debugger_capture(&self) {
        // WebGPU has no capture API.
    }

    /// `Wait` delivers every callback owed, each once the GPU has done its work, whatever
    /// submission or timeout it names: the host waits on all of the device's work, with no limit.
    /// `Poll` delivers none, since nothing here learns whether the GPU is done without waiting;
    /// its callbacks run as the program yields.
    fn poll(
        &self,
        poll_type: wgpu::wgt::PollType<u64>,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        match poll_type {
            wgpu::wgt::PollType::Wait { .. } => {
                self.owed.deliver();
                Ok(wgpu::PollStatus::WaitSucceeded)
            }
            wgpu::wgt::PollType::Poll => Ok(wgpu::PollStatus::Poll),
        }
    }

    fn get_internal_counters(&self) -> wgpu::InternalCounters {
        wgpu::InternalCounters::default()
    }

    fn generate_allocator_report(&self) -> Option<wgpu::AllocatorReport> {
        None
    }

    fn destroy(&self) {
        self.device.destroy();
    }
}
