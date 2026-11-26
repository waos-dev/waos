//! Shader modules. WGSL, SPIR-V and GLSL go to the engine as they are, where naga reads and
//! validates them; naga's own module (`ShaderSource::Naga`), which has no form that stays stable
//! across naga's versions, is written as WGSL here first, as wgpu's backend for the web writes it.
//!
//! The invariant: a module that cannot be written as WGSL still makes a module, an invalid one
//! whose compilation info says why, as an invalid source does in the engine; it never stops the
//! program.

use std::pin::Pin;
use std::sync::Arc;

use wasdk::gpu::{Device, ShaderModule};
use wasdk::gpu_types::{ShaderModuleDescriptor, ShaderSource};
use wgpu::custom::{DispatchShaderModule, ShaderCompilationInfoFuture, ShaderModuleInterface};
use wgpu::naga;

use crate::convert;
use crate::left_out::left_out;

/// The module behind a [`wgpu::ShaderModule`] from this backend.
#[derive(Debug)]
pub struct WasdkShaderModule {
    module: Arc<ShaderModule>,
    /// Why naga's module could not be written as WGSL: the one message of its compilation. None
    /// for a module the engine read, whose messages are the engine's.
    unwritten: Option<String>,
}

downcast!(WasdkShaderModule: api wgpu::ShaderModule);

impl WasdkShaderModule {
    pub(crate) fn wit_shader_module(&self) -> &ShaderModule {
        &self.module
    }
}

pub(crate) fn create_shader_module(
    device: &Device,
    desc: &wgpu::ShaderModuleDescriptor<'_>,
) -> DispatchShaderModule {
    let source = match &desc.source {
        wgpu::ShaderSource::Wgsl(code) => ShaderSource::Wgsl(code.to_string()),
        wgpu::ShaderSource::Naga(module) => match wgsl_of(module) {
            Ok(code) => ShaderSource::Wgsl(code),
            Err(why) => return unwritten(device, desc.label, why),
        },
        #[cfg(feature = "spirv")]
        wgpu::ShaderSource::SpirV(words) => ShaderSource::Spirv(words.to_vec()),
        #[cfg(feature = "glsl")]
        wgpu::ShaderSource::Glsl {
            shader,
            stage,
            defines,
        } => ShaderSource::Glsl(wasdk::gpu_types::GlslSource {
            code: shader.to_string(),
            stage: shader_stage_to_wit(*stage),
            defines: (defines.iter())
                .map(|&(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        }),
        // `ShaderSource` is `#[non_exhaustive]`; SPIR-V and GLSL sit behind wgpu's features, read
        // here with this crate's of the same names.
        _ => left_out(
            "SPIR-V and GLSL shader sources without this crate's `spirv` or `glsl` feature",
        ),
    };
    DispatchShaderModule::custom(WasdkShaderModule {
        module: Arc::new(engine_module(device, desc.label, source)),
        unwritten: None,
    })
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

/// A GLSL shader's stage; the mesh and ray tracing stages are left out.
#[cfg(feature = "glsl")]
fn shader_stage_to_wit(stage: naga::ShaderStage) -> wasdk::gpu_types::ShaderStage {
    use wasdk::gpu_types::ShaderStage;
    match stage {
        naga::ShaderStage::Vertex => ShaderStage::Vertex,
        naga::ShaderStage::Fragment => ShaderStage::Fragment,
        naga::ShaderStage::Compute => ShaderStage::Compute,
        other => left_out(format_args!("the shader stage {other:?}")),
    }
}

/// An invalid module, whose compilation says `why`.
fn unwritten(device: &Device, label: wgpu::Label<'_>, why: String) -> DispatchShaderModule {
    DispatchShaderModule::custom(WasdkShaderModule {
        module: Arc::new(engine_module(
            device,
            label,
            ShaderSource::Wgsl(String::new()),
        )),
        unwritten: Some(why),
    })
}

fn engine_module(device: &Device, label: wgpu::Label<'_>, source: ShaderSource) -> ShaderModule {
    device.create_shader_module(&ShaderModuleDescriptor {
        label: convert::label(label),
        source,
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
                None => convert::compilation_info_from_wit(module.compilation_info().await),
            }
        })
    }
}
