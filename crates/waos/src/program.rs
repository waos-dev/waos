//! A program as the engine loaded it: compiled, validated, and bound to the engine that did.

use wasmtime::component::Component;

/// A program the engine loaded, which it runs any number of times.
pub struct Program {
    component: Component,
    /// What names the program in a trace, in what it prints, and to the shell: its file's name
    /// without the extension, unless the embedder names it.
    name: String,
}

impl Program {
    pub(crate) fn new(component: Component, name: String) -> Program {
        Program { component, name }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The same program, named `name` rather than by its file: an installed program by its id,
    /// where every install's component file has one name.
    pub fn named(self, name: impl Into<String>) -> Program {
        Program {
            name: name.into(),
            ..self
        }
    }

    pub(crate) fn component(&self) -> &Component {
        &self.component
    }
}
