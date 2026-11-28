//! A program as the engine loaded it: compiled, validated, and bound to the engine that did.

use waos_capabilities::Role;
use wasmtime::component::Component;

use crate::roles;
use crate::source_lines::ComponentFile;

/// A program the engine loaded, which it runs any number of times.
pub struct Program {
    component: Component,
    /// What names the program in a trace, in what it prints, and to the shell: its file's name
    /// without the extension, unless the embedder names it.
    name: String,
    /// The file it was loaded from, where a failure's backtrace is located in its source.
    file: ComponentFile,
}

impl Program {
    pub(crate) fn new(component: Component, name: String, file: ComponentFile) -> Program {
        Program {
            component,
            name,
            file,
        }
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

    /// The roles it asks for, by the interfaces it imports: what its user is asked to grant as it
    /// starts, and what whoever runs it hands it the providers of.
    pub fn roles(&self) -> Vec<Role> {
        roles::asked_for(&self.component, self.component.engine())
    }

    pub(crate) fn component(&self) -> &Component {
        &self.component
    }

    pub(crate) fn file(&self) -> &ComponentFile {
        &self.file
    }
}
