//! Shader modules. The boundary carries WGSL text alone, as WebGPU does, so a shader given in
//! another form is written as WGSL first, as wgpu's own backend for the web writes it: naga's
//! module (`ShaderSource::Naga`), and, with this crate's `spirv` and `glsl` features, SPIR-V and
//! GLSL, which naga reads into a module.
//!
//! The invariant: a source that cannot be written as WGSL still makes a module, an invalid one
//! whose compilation info says why, as the web's does; it never stops the program.

use std::pin::Pin;
use std::sync::Arc;

use wasdk::webgpu::{GpuDevice, GpuShaderModule, GpuShaderModuleDescriptor};
use wgpu::custom::{DispatchShaderModule, ShaderCompilationInfoFuture, ShaderModuleInterface};
use wgpu::naga;

use crate::convert;
use crate::gap::wasi_gap;

#[derive(Debug)]
pub struct WasdkShaderModule {
    module: Arc<GpuShaderModule>,
    /// Why its source, given in another form, could not be written as WGSL: the one message of
    /// its compilation. None for a source that was, whose messages are the host's.
    unwritten: Option<String>,
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
        wgpu::ShaderSource::Naga(module) => compile_written(device, wgsl_of(module), desc.label),
        #[cfg(feature = "spirv")]
        wgpu::ShaderSource::SpirV(words) => {
            let written = module_of_spirv(words).and_then(|module| wgsl_of(&module));
            compile_written(device, written, desc.label)
        }
        #[cfg(feature = "glsl")]
        wgpu::ShaderSource::Glsl {
            shader,
            stage,
            defines,
        } => {
            let written =
                module_of_glsl(shader, *stage, defines).and_then(|module| wgsl_of(&module));
            compile_written(device, written, desc.label)
        }
        // `ShaderSource` is `#[non_exhaustive]`; SPIR-V and GLSL sit behind wgpu's features, read
        // here with this crate's of the same names.
        _ => wasi_gap(
            "SPIR-V and GLSL shader sources without this crate's `spirv` or `glsl` feature",
        ),
    }
}

/// `module` written as WGSL, once it validates; why not, where it does not.
fn wgsl_of(module: &naga::Module) -> Result<String, String> {
    use naga::valid::{Capabilities, ValidationFlags, Validator};
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(module)
        .map_err(|error| error.emit_to_string(""))?;
    naga::back::wgsl::write_string(module, &info, naga::back::wgsl::WriterFlags::empty())
        .map_err(|error| error.to_string())
}

/// The module SPIR-V's `words` are, read as wgpu's backend for the web reads them.
#[cfg(feature = "spirv")]
fn module_of_spirv(words: &[u32]) -> Result<naga::Module, String> {
    let options = naga::front::spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: true,
        block_ctx_dump_prefix: None,
    };
    naga::front::spv::Frontend::new(words.iter().copied(), &options)
        .parse()
        .map_err(|error| error.to_string())
}

/// The module the GLSL `shader` of `stage` is, with `defines`.
#[cfg(feature = "glsl")]
fn module_of_glsl(
    shader: &str,
    stage: naga::ShaderStage,
    defines: &[(&str, &str)],
) -> Result<naga::Module, String> {
    let options = naga::front::glsl::Options {
        stage,
        defines: (defines.iter())
            .map(|&(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
    };
    naga::front::glsl::Frontend::default()
        .parse(&options, shader)
        .map_err(|errors| errors.emit_to_string(shader))
}

/// A module of the WGSL `written` from a source in another form; an invalid one that says why,
/// where it could not be written.
fn compile_written(
    device: &GpuDevice,
    written: Result<String, String>,
    label: wgpu::Label<'_>,
) -> DispatchShaderModule {
    match written {
        Ok(code) => compile(device, &code, label),
        Err(why) => DispatchShaderModule::custom(WasdkShaderModule {
            module: Arc::new(host_module(device, "", label)),
            unwritten: Some(why),
        }),
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
    DispatchShaderModule::custom(WasdkShaderModule {
        module: Arc::new(host_module(device, code, label)),
        unwritten: None,
    })
}

/// The host's module of the WGSL `code`.
fn host_module(device: &GpuDevice, code: &str, label: wgpu::Label<'_>) -> GpuShaderModule {
    device.create_shader_module(&GpuShaderModuleDescriptor {
        code: code.to_owned(),
        compilation_hints: None,
        label: convert::label(label),
    })
}

impl ShaderModuleInterface for WasdkShaderModule {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        let (module, unwritten) = (Arc::clone(&self.module), self.unwritten.clone());
        Box::pin(async move {
            match unwritten {
                Some(why) => wgpu::CompilationInfo {
                    messages: vec![wgpu::CompilationMessage {
                        message: why,
                        message_type: wgpu::CompilationMessageType::Error,
                        location: None,
                    }],
                },
                None => convert::compilation_info_from_wasi(&module.get_compilation_info().await),
            }
        })
    }
}
