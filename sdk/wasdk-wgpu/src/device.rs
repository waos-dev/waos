//! The device: every object it creates, its error scopes, its polls, and what happens to it.

use std::cell::RefCell;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use wasdk::gpu::Device;
use wgpu::custom::{
    BoxDeviceLostCallback, DeviceInterface, DispatchBindGroup, DispatchBindGroupLayout,
    DispatchBlas, DispatchBuffer, DispatchCommandEncoder, DispatchComputePipeline, DispatchDevice,
    DispatchExternalTexture, DispatchPipelineCache, DispatchPipelineLayout, DispatchQuerySet,
    DispatchRenderBundleEncoder, DispatchRenderPipeline, DispatchSampler, DispatchShaderModule,
    DispatchTexture, DispatchTlas, PopErrorScopeFuture,
};
use wgpu::{AdapterInfo, Features, Limits};

use crate::left_out::left_out;
use crate::runtime::DeviceCallbacks;
use crate::{binding, buffer, command, convert, pass, pipeline, query, sampler, shader, texture};

/// The device behind a [`wgpu::Device`] from this backend: `device.as_custom::<WasdkDevice>()`.
#[derive(Debug)]
pub struct WasdkDevice {
    device: Arc<Device>,
    /// What it answers the program through, shared with its queue.
    callbacks: Arc<DeviceCallbacks>,
    /// How many error scopes are open, so a pop out of order is caught here.
    open_error_scopes: AtomicU32,
}

downcast!(WasdkDevice: api wgpu::Device, dispatch DispatchDevice);

impl WasdkDevice {
    /// The device over the engine's, with `callbacks`, which are `device`'s.
    pub(crate) fn new(device: Device, callbacks: Arc<DeviceCallbacks>) -> Self {
        let device = Arc::new(device);
        LIVE_DEVICES.with_borrow_mut(|live| {
            live.push((Arc::downgrade(&device), Arc::downgrade(&callbacks)));
        });
        WasdkDevice {
            device,
            callbacks,
            open_error_scopes: AtomicU32::new(0),
        }
    }

    /// The engine's device behind the wgpu one, which a view's surface is configured on and a
    /// hardware buffer is imported on as a texture.
    pub fn wit_device(&self) -> &Device {
        &self.device
    }
}

thread_local! {
    /// The devices the program made and has not dropped, with their callbacks, which
    /// [`poll_all`] polls. Held weakly, so a device dropped drops its callbacks.
    static LIVE_DEVICES: RefCell<Vec<(Weak<Device>, Weak<DeviceCallbacks>)>> =
        const { RefCell::new(Vec::new()) };
}

/// Polls `device` as `poll_type` says, then runs the callbacks whose answers came.
fn poll(
    device: &Device,
    callbacks: &DeviceCallbacks,
    poll_type: wgpu::wgt::PollType<u64>,
) -> Result<wgpu::PollStatus, wgpu::PollError> {
    let polled = device.poll(convert::poll_type_to_wit(poll_type));
    callbacks.run_ready();
    let status = polled.map_err(convert::poll_error_from_wit)?;
    Ok(convert::poll_status_from_wit(status))
}

/// Polls every device the program made and still holds, waiting on each where `force_wait`, as
/// native wgpu's `poll_all_devices` polls every device of its instance; whether every device's
/// queue is empty. A failed poll is fatal, as it is there.
pub(crate) fn poll_all(force_wait: bool) -> bool {
    let devices: Vec<_> = LIVE_DEVICES.with_borrow_mut(|live| {
        live.retain(|(device, _)| device.strong_count() > 0);
        live.iter()
            .filter_map(|(device, callbacks)| Some((device.upgrade()?, callbacks.upgrade()?)))
            .collect()
    });
    let poll_type = if force_wait {
        wgpu::wgt::PollType::Wait {
            submission_index: None,
            timeout: None,
        }
    } else {
        wgpu::wgt::PollType::Poll
    };
    devices.iter().fold(true, |all_empty, (device, callbacks)| {
        let status = poll(device, callbacks, poll_type.clone())
            .unwrap_or_else(|error| panic!("Instance::poll_all_devices: {error}"));
        all_empty && status.is_queue_empty()
    })
}

// `create_shader_module_passthrough`, `create_pipeline_cache` and the debugger-capture pair
// are `unsafe` in the trait; nothing they do here is.
#[allow(unsafe_code)]
impl DeviceInterface for WasdkDevice {
    fn features(&self) -> Features {
        convert::features_from_wit(self.device.features())
    }

    fn limits(&self) -> Limits {
        convert::limits_from_wit(&self.device.limits())
    }

    fn adapter_info(&self) -> AdapterInfo {
        convert::adapter_info_from_wit(self.device.adapter_info())
    }

    /// naga's bounds checks always run in the engine, whatever `runtime_checks` asks.
    fn create_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
        _runtime_checks: wgpu::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        shader::create_shader_module(&self.device, &desc)
    }

    unsafe fn create_shader_module_passthrough(
        &self,
        _desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        left_out("passthrough shaders (Device::create_shader_module_passthrough)")
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
        left_out("mesh shaders (Device::create_mesh_pipeline)")
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
        left_out("pipeline caches (Device::create_pipeline_cache), which the engine keeps")
    }

    fn create_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> DispatchBuffer {
        buffer::create_buffer(&self.device, &self.callbacks.pending, desc)
    }

    fn create_texture(&self, desc: &wgpu::TextureDescriptor<'_>) -> DispatchTexture {
        texture::create_texture(&self.device, desc)
    }

    fn create_external_texture(
        &self,
        desc: &wgpu::ExternalTextureDescriptor<'_>,
        planes: &[&wgpu::TextureView],
    ) -> DispatchExternalTexture {
        texture::create_external_texture(&self.device, desc, planes)
    }

    fn create_blas(
        &self,
        _desc: &wgpu::CreateBlasDescriptor<'_>,
        _sizes: wgpu::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        left_out("ray tracing (Device::create_blas)")
    }

    fn create_tlas(&self, _desc: &wgpu::CreateTlasDescriptor<'_>) -> DispatchTlas {
        left_out("ray tracing (Device::create_tlas)")
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
        self.callbacks.device_lost.set(callback);
    }

    fn on_uncaptured_error(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {
        self.callbacks.uncaptured_errors.set_handler(handler);
    }

    fn push_error_scope(&self, filter: wgpu::ErrorFilter) -> u32 {
        let index = self.open_error_scopes.fetch_add(1, Ordering::Relaxed);
        self.device
            .push_error_scope(convert::error_filter_to_wit(filter));
        index
    }

    /// Scopes pop in reverse order, as wgpu requires; a pop out of order panics here, where the
    /// engine could only see a pop of the innermost. The scope pops at the call, as wgpu's
    /// `ErrorScopeGuard` promises: its drop never polls the future, and what the program does
    /// before it awaits one is outside the scope.
    fn pop_error_scope(&self, index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        let open = self.open_error_scopes.load(Ordering::Relaxed);
        assert!(
            open == index + 1,
            "wasdk-wgpu: error scope {index} popped with {open} open; scopes pop in reverse order"
        );
        self.open_error_scopes.store(index, Ordering::Relaxed);
        let error = self.device.pop_error_scope();
        Box::pin(std::future::ready(error.map(convert::error_from_wit)))
    }

    unsafe fn start_graphics_debugger_capture(&self) {
        // The engine's development tools start captures; a program cannot.
    }

    unsafe fn stop_graphics_debugger_capture(&self) {
        // The engine's development tools start captures; a program cannot.
    }

    /// The engine's poll, its submission and timeout honoured; then every callback whose work
    /// is done by now, in the order asked, as wgpu-core runs them inside its poll, whether it
    /// waited or timed out, and the device's uncaptured errors and loss.
    fn poll(
        &self,
        poll_type: wgpu::wgt::PollType<u64>,
    ) -> Result<wgpu::PollStatus, wgpu::PollError> {
        poll(&self.device, &self.callbacks, poll_type)
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
