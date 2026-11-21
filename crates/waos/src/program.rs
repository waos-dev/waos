//! A program as the engine loaded it: compiled, validated, and bound to the engine that did.

use wasmtime::component::Component;

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

    /// Whether the program has no UI, a console program: its component imports no
    /// `wasdk:platform/views`, read from the component alone, as its roles are (decision 3 of
    /// `docs/proposals/2026-11-21-views-a-program-asks-for.md`). wasdk imports only what a program
    /// calls, so a program that imports `views` reaches for a view, and every framework's adapter
    /// does; `wasi:cli` tells nothing, since `std` imports it for every program.
    pub fn is_console_program(&self) -> bool {
        !self
            .component
            .component_type()
            .imports(self.component.engine())
            .any(|(name, _)| is_views_interface(name))
    }

    pub(crate) fn component(&self) -> &Component {
        &self.component
    }

    pub(crate) fn file(&self) -> &ComponentFile {
        &self.file
    }
}

/// Whether the import `name`, `wasdk:platform/views@2.0.0`, is the views interface at any version.
fn is_views_interface(name: &str) -> bool {
    name.split_once('@')
        .map_or(name, |(interface, _)| interface)
        == VIEWS
}

/// The interface a program draws through, which a program with a UI imports.
const VIEWS: &str = "wasdk:platform/views";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_views_interface_is_known_at_any_version_and_no_other_is() {
        assert!(is_views_interface("wasdk:platform/views@2.0.0"));
        assert!(is_views_interface("wasdk:platform/views@1.0.0"));
        assert!(is_views_interface("wasdk:platform/views"));
        assert!(!is_views_interface("wasdk:platform/windows@2.0.0"));
        assert!(!is_views_interface("wasdk:platform/views-extra@2.0.0"));
        assert!(!is_views_interface("wasi:cli/stdout@0.3.0"));
    }
}
