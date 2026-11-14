//! A shader module's compilation info: the messages its compilation gave, which WebGPU's
//! `getCompilationInfo` answers. https://www.w3.org/TR/webgpu/#dom-gpushadermodule-getcompilationinfo
//!
//! wgpu-core tells of a compilation only through the module's creation error, so the host keeps
//! the messages from it when the module is made. A message's position is in UTF-16 code units, as
//! WebGPU gives it, counted from naga's bytes.

use std::sync::Arc;

use wasmtime::component::{Accessor, Resource};
use wgpu_core::naga::SourceLocation;
use wgpu_core::pipeline::CreateShaderModuleError;

use crate::gpu_object::GpuObject;
use crate::wasi::webgpu::webgpu;
use crate::WasiWebGpuCtx;

/// A shader module as the program holds it: its object, and what its compilation said.
pub struct ShaderModule {
    pub(crate) module: GpuObject<wgpu_core::id::ShaderModuleId>,
    pub(crate) compilation: CompilationInfo,
}

impl ShaderModule {
    pub(crate) fn id(&self) -> wgpu_core::id::ShaderModuleId {
        self.module.id()
    }
}

/// The messages of one compilation, shared by every `get-compilation-info` of the module.
#[derive(Clone, Default)]
pub struct CompilationInfo(Arc<[CompilationMessage]>);

/// One message of a compilation, where in the source it points, and nothing where it points
/// nowhere, as WebGPU's are.
#[derive(Clone)]
pub struct CompilationMessage {
    pub(crate) message: String,
    pub(crate) type_: webgpu::GpuCompilationMessageType,
    pub(crate) line_num: u64,
    pub(crate) line_pos: u64,
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

impl CompilationInfo {
    /// What the compilation that ended in `error` said: nothing when it succeeded, or failed for
    /// a reason that is not the shader's, which the device reports instead.
    pub(crate) fn of(error: Option<&CreateShaderModuleError>) -> CompilationInfo {
        let Some(error) = error else {
            return CompilationInfo::default();
        };
        let located = match error {
            CreateShaderModuleError::Parsing(parsing) => parsing
                .inner
                .location(&parsing.source)
                .map(|at| (at, &parsing.source)),
            CreateShaderModuleError::Validation(validation) => validation
                .inner
                .location(&validation.source)
                .map(|at| (at, &validation.source)),
            CreateShaderModuleError::Device(_) | CreateShaderModuleError::Generation => {
                return CompilationInfo::default();
            }
            _ => None,
        };
        let message = match located {
            Some((at, source)) => CompilationMessage::error_at(error.to_string(), at, source),
            None => CompilationMessage::error_at_nowhere(error.to_string()),
        };
        CompilationInfo(Arc::new([message]))
    }

    pub(crate) fn messages(&self) -> &[CompilationMessage] {
        &self.0
    }
}

impl CompilationMessage {
    fn error_at(message: String, at: SourceLocation, source: &str) -> CompilationMessage {
        let start = at.offset as usize;
        let end = start + at.length as usize;
        let line_start = source
            .get(..start)
            .and_then(|before| before.rfind('\n'))
            .map_or(0, |newline| newline + 1);
        let in_utf16 = |from: usize, to: usize| source.get(from..to).map(utf16_length);
        match (
            in_utf16(line_start, start),
            in_utf16(0, start),
            in_utf16(start, end),
        ) {
            (Some(column), Some(offset), Some(length)) => CompilationMessage {
                message,
                type_: webgpu::GpuCompilationMessageType::Error,
                line_num: u64::from(at.line_number),
                line_pos: column + 1,
                offset,
                length,
            },
            // A span that does not fall on characters of the source points nowhere in it.
            _ => CompilationMessage::error_at_nowhere(message),
        }
    }

    fn error_at_nowhere(message: String) -> CompilationMessage {
        CompilationMessage {
            message,
            type_: webgpu::GpuCompilationMessageType::Error,
            line_num: 0,
            line_pos: 0,
            offset: 0,
            length: 0,
        }
    }
}

fn utf16_length(text: &str) -> u64 {
    text.encode_utf16().count() as u64
}

impl<T: Send> webgpu::HostGpuShaderModuleWithStore<T> for crate::HasWasiWebGpuCtx {
    async fn get_compilation_info(
        accessor: &Accessor<T, Self>,
        shader: Resource<webgpu::GpuShaderModule>,
    ) -> wasmtime::Result<Resource<webgpu::GpuCompilationInfo>> {
        accessor.with(|mut access| {
            let ctx = access.get();
            let compilation = ctx.table.get(&shader)?.compilation.clone();
            Ok(ctx.table.push(compilation)?)
        })
    }
}

impl<'a> webgpu::HostGpuCompilationInfo for WasiWebGpuCtx<'a> {
    fn messages(
        &mut self,
        info: Resource<webgpu::GpuCompilationInfo>,
    ) -> wasmtime::Result<Vec<Resource<webgpu::GpuCompilationMessage>>> {
        let messages = self.table.get(&info)?.messages().to_vec();
        messages
            .into_iter()
            .map(|message| Ok(self.table.push(message)?))
            .collect()
    }

    fn drop(&mut self, info: Resource<webgpu::GpuCompilationInfo>) -> wasmtime::Result<()> {
        self.table.delete(info)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuCompilationMessage for WasiWebGpuCtx<'a> {
    fn message(
        &mut self,
        message: Resource<webgpu::GpuCompilationMessage>,
    ) -> wasmtime::Result<String> {
        Ok(self.table.get(&message)?.message.clone())
    }

    fn type_(
        &mut self,
        message: Resource<webgpu::GpuCompilationMessage>,
    ) -> wasmtime::Result<webgpu::GpuCompilationMessageType> {
        Ok(self.table.get(&message)?.type_)
    }

    fn line_num(
        &mut self,
        message: Resource<webgpu::GpuCompilationMessage>,
    ) -> wasmtime::Result<u64> {
        Ok(self.table.get(&message)?.line_num)
    }

    fn line_pos(
        &mut self,
        message: Resource<webgpu::GpuCompilationMessage>,
    ) -> wasmtime::Result<u64> {
        Ok(self.table.get(&message)?.line_pos)
    }

    fn offset(
        &mut self,
        message: Resource<webgpu::GpuCompilationMessage>,
    ) -> wasmtime::Result<u64> {
        Ok(self.table.get(&message)?.offset)
    }

    fn length(
        &mut self,
        message: Resource<webgpu::GpuCompilationMessage>,
    ) -> wasmtime::Result<u64> {
        Ok(self.table.get(&message)?.length)
    }

    fn drop(&mut self, message: Resource<webgpu::GpuCompilationMessage>) -> wasmtime::Result<()> {
        self.table.delete(message)?;
        Ok(())
    }
}
