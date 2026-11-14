//! A program the engine cannot link: one built for another version of the interface.
//!
//! The invariant: the failure is told apart where it happens, by the step that failed, never by
//! the words of the error. Linking a component is wasmtime's `Linker::instantiate_pre`, which
//! checks every import against what the linker has and does nothing else (the one other thing it
//! refuses, a component of another engine, `WaOS::run` refuses first), so every error of that step
//! is this one and no other step's is.

use std::error::Error;
use std::fmt;

/// The program asks of WaOS something this WaOS does not offer in that shape: an interface, a
/// function or a type it imports is missing from what the engine serves, or differs from it, as a
/// variant with a case more or less, or a record with another field. It was built for another
/// version of the interface, older or newer, and nothing of it ran. What a failed
/// [`ProgramEnd`](super::ProgramEnd) carries in its error's chain, where `downcast_ref` finds it.
#[derive(Debug)]
pub struct IncompatibleProgram {
    detail: String,
}

impl IncompatibleProgram {
    /// The program could not be linked, as `link_error` says.
    pub(crate) fn new(link_error: &wasmtime::Error) -> IncompatibleProgram {
        IncompatibleProgram {
            detail: format!("{link_error:#}"),
        }
    }

    /// wasmtime's own words: which import did not match, and how.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for IncompatibleProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the program was built for another version of the interface: {}",
            self.detail
        )
    }
}

impl Error for IncompatibleProgram {}
