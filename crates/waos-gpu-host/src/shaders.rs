//! The shader module: its source read and validated by naga in wgpu-core, and the messages that
//! gave, which wgpu-core tells only through the module's creation error and the module keeps.

use wasmtime::component::{Accessor, Resource};
use wgpu_core::id::ShaderModuleId;
use wgpu_core::pipeline::CreateShaderModuleError;

use crate::conversions::label;
use crate::devices::Device;
use crate::wasdk::gpu::gpu::{self, CompilationMessage, ShaderModuleDescriptor};
use crate::wasdk::gpu::types::{CompilationMessageType, SourceLocation};
use crate::{GpuCtx, GpuObject, HasGpuCtx};

/// A shader module as the program holds it, and what its compilation said.
pub struct ShaderModule {
    object: GpuObject<ShaderModuleId>,
    messages: Vec<CompilationMessage>,
}

impl ShaderModule {
    /// The module `descriptor` describes, on `device`, with full runtime checks: none of naga's
    /// bounds checks is turned off.
    pub(crate) fn create(device: &Device, descriptor: ShaderModuleDescriptor) -> ShaderModule {
        let instance = device.instance();
        let core_descriptor = wgpu_core::pipeline::ShaderModuleDescriptor {
            label: label(descriptor.label),
            runtime_checks: wgpu_types::ShaderRuntimeChecks::checked(),
        };
        let (module, error) = instance.device_create_shader_module(
            device.device_id(),
            &core_descriptor,
            descriptor.source.into(),
            None,
        );
        let messages = error.as_ref().map(messages_of).unwrap_or_default();
        device.errors.report(
            error,
            "Device::create_shader_module",
            core_descriptor.label.as_deref(),
        );
        ShaderModule {
            object: GpuObject::new(module, instance),
            messages,
        }
    }

    pub(crate) fn id(&self) -> ShaderModuleId {
        self.object.id()
    }
}

/// What the compilation that ended in `error` said, as wgpu tells it: nothing for a failure that
/// is not the shader's, which the device reports instead.
fn messages_of(error: &CreateShaderModuleError) -> Vec<CompilationMessage> {
    let located =
        |message: String, location: Option<wgpu_core::naga::SourceLocation>| CompilationMessage {
            message,
            message_type: CompilationMessageType::Error,
            location: location.map(|at| SourceLocation {
                line_number: at.line_number,
                line_position: at.line_position,
                offset: at.offset,
                length: at.length,
            }),
        };
    match error {
        CreateShaderModuleError::Parsing(parsing) => vec![located(
            parsing.to_string(),
            parsing.inner.location(&parsing.source),
        )],
        CreateShaderModuleError::ParsingGlsl(parsing) => parsing
            .inner
            .errors
            .iter()
            .map(|error| located(error.to_string(), error.location(&parsing.source)))
            .collect(),
        CreateShaderModuleError::ParsingSpirV(parsing) => vec![located(parsing.to_string(), None)],
        CreateShaderModuleError::Validation(validation) => vec![located(
            validation.to_string(),
            validation.inner.location(&validation.source),
        )],
        CreateShaderModuleError::Device(_) | CreateShaderModuleError::Generation => Vec::new(),
        other => vec![located(other.to_string(), None)],
    }
}

impl gpu::HostShaderModule for GpuCtx<'_> {
    fn drop(&mut self, module: Resource<ShaderModule>) -> wasmtime::Result<()> {
        self.table.delete(module)?;
        Ok(())
    }
}

impl<T: Send> gpu::HostShaderModuleWithStore<T> for HasGpuCtx {
    async fn compilation_info(
        accessor: &Accessor<T, Self>,
        module: Resource<ShaderModule>,
    ) -> wasmtime::Result<Vec<CompilationMessage>> {
        accessor.with(|mut access| Ok(access.get().table.get(&module)?.messages.clone()))
    }
}
