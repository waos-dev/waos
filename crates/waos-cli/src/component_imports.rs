//! Which interfaces a component imports, read from its own import section: what `waos package`
//! checks an app against (decision 3 of `docs/proposals/2026-11-21-views-a-program-asks-for.md`).
//!
//! The invariant: only the component's own imports count, not those of the modules and components
//! nested in it, which its own satisfy.

use anyhow::{Context, Result};
use wasmparser::{Parser, Payload};

/// The interface a program draws through, which an app's component imports.
const VIEWS: &str = "wasdk:platform/views";

/// Whether `component`, a component's bytes, imports `wasdk:platform/views`, at any version.
pub fn imports_views(component: &[u8]) -> Result<bool> {
    Ok(top_level_imports(component)?
        .iter()
        .any(|name| interface_of(name) == VIEWS))
}

/// The names the component itself imports, `wasdk:platform/views@2.2.0` among them.
fn top_level_imports(component: &[u8]) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut depth = 0usize;
    for payload in Parser::new(0).parse_all(component) {
        match payload.context("reading the component")? {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
            Payload::End(_) => depth = depth.saturating_sub(1),
            Payload::ComponentImportSection(imports) if depth == 0 => {
                for import in imports {
                    names.push(import.context("reading an import")?.name.name.to_owned());
                }
            }
            _ => {}
        }
    }
    Ok(names)
}

/// The interface an import names, without its version.
fn interface_of(name: &str) -> &str {
    name.split_once('@')
        .map_or(name, |(interface, _)| interface)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interface_is_named_at_any_version() {
        assert_eq!(interface_of("wasdk:platform/views@2.0.0"), VIEWS);
        assert_eq!(interface_of(VIEWS), VIEWS);
        assert_ne!(interface_of("wasdk:platform/views-extra@2.0.0"), VIEWS);
    }

    #[test]
    fn a_component_importing_views_is_told_from_one_that_does_not() {
        let drawing =
            wat::parse_str(r#"(component (import "wasdk:platform/views@2.0.0" (instance)))"#)
                .unwrap();
        let printing =
            wat::parse_str(r#"(component (import "wasi:cli/stdout@0.3.0" (instance)))"#).unwrap();
        assert!(imports_views(&drawing).unwrap());
        assert!(!imports_views(&printing).unwrap());
    }

    #[test]
    fn a_nested_components_import_is_not_the_components_own() {
        let nested = wat::parse_str(
            r#"(component (component (import "wasdk:platform/views@2.0.0" (instance))))"#,
        )
        .unwrap();
        assert!(!imports_views(&nested).unwrap());
    }
}
