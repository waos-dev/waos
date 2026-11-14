//! Shader modules, from WGSL text only.

use std::pin::Pin;
use std::sync::Arc;

use wasdk::webgpu::{GpuDevice, GpuShaderModule, GpuShaderModuleDescriptor};
use wgpu::custom::{DispatchShaderModule, ShaderCompilationInfoFuture, ShaderModuleInterface};

use crate::convert;
use crate::gap::wasi_gap;

#[derive(Debug)]
pub struct WasdkShaderModule {
    module: Arc<GpuShaderModule>,
}

downcast!(WasdkShaderModule: api wgpu::ShaderModule);

impl WasdkShaderModule {
    pub(crate) fn gpu_shader_module(&self) -> &GpuShaderModule {
        &self.module
    }
}

pub(crate) fn create_shader_module(
    device: &GpuDevice,
    desc: &wgpu::ShaderModuleDescriptor<'_>,
) -> DispatchShaderModule {
    match &desc.source {
        wgpu::ShaderSource::Wgsl(code) => compile(device, code, desc.label),
        // `ShaderSource` is `#[non_exhaustive]`; SPIR-V, GLSL and naga IR sit behind wgpu
        // features and have no wasi form either way.
        _ => wasi_gap("shader sources other than WGSL (ShaderSource::{SpirV, Glsl, Naga})"),
    }
}

pub(crate) fn create_shader_module_passthrough(
    device: &GpuDevice,
    desc: &wgpu::ShaderModuleDescriptorPassthrough<'_>,
) -> DispatchShaderModule {
    match &desc.wgsl {
        Some(code) => compile(device, code, desc.label),
        None => wasi_gap("passthrough shaders in a language other than WGSL"),
    }
}

fn compile(device: &GpuDevice, code: &str, label: wgpu::Label<'_>) -> DispatchShaderModule {
    let descriptor = GpuShaderModuleDescriptor {
        code: code.to_owned(),
        compilation_hints: None,
        label: convert::label(label),
    };
    DispatchShaderModule::custom(WasdkShaderModule {
        module: Arc::new(device.create_shader_module(&descriptor)),
    })
}

impl ShaderModuleInterface for WasdkShaderModule {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        let module = Arc::clone(&self.module);
        Box::pin(async move {
            convert::compilation_info_from_wasi(&module.get_compilation_info().await)
        })
    }
}
