//! The engine's compiler: what compiles a component into its code folder, made before the engine
//! and handed to it ([`WaOSConfig::compiler`](crate::WaOSConfig::compiler)), so whoever compiles
//! apps' code at their install is handed it without needing the engine, which is made later from
//! what those same parts serve.
//!
//! The invariant: an engine maps the code its own compiler writes, since they share one wasmtime
//! engine; two compilers made apart are two engines, whose code each maps only where their
//! settings match.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use wasmtime::Engine;

use crate::code_folder;
use crate::engine::engine_config;

/// Compiles components into their code folders, for the engine made with it. Clones share it.
#[derive(Clone)]
pub struct Compiler {
    engine: Arc<Engine>,
}

impl Compiler {
    /// A compiler, compiling with native unwind information where `native_unwind_info` asks
    /// (wasmtime's `Config::native_unwind_info`): what a native profiler or debugger needs to walk
    /// through a program's frames, and nothing else does, wasmtime's own backtraces of a program
    /// among them. Off unless asked for: macOS's unwinder lets go of each compiled function's
    /// information one at a time, at a cost that grows with the square of their number, so a
    /// large program takes seconds to drop with it (the gallery's debug build, 7 s against 2 ms).
    /// Always on Windows, whose exception handling needs it, and where wasmtime refuses it off.
    pub fn new(native_unwind_info: bool) -> Result<Compiler> {
        Ok(Compiler {
            engine: Arc::new(Engine::new(&engine_config(native_unwind_info))?),
        })
    }

    /// Whether `code`, a component's code folder, holds the code this compiler writes, so that
    /// the engine's `load` maps it without compiling.
    pub fn is_compiled(&self, code: &Path) -> bool {
        code_folder::is_compiled(&self.engine, code)
    }

    /// Compiles the component at `component` into `code`, its code folder, as an app compiles a
    /// program it installs, so that the engine's `load` maps it without compiling; the code other
    /// compilers wrote there goes. Blocks for seconds, so call it off the main thread.
    pub fn compile(&self, component: &Path, code: &Path) -> Result<()> {
        code_folder::compile(&self.engine, component, code)
    }

    /// The wasmtime engine it compiles for, which the engine made with it runs on.
    pub(crate) fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }
}
