//! The example programs the engine's tests run, built as components by this crate's build
//! script: the path of each, as a constant named after it; and each one's code folder
//! ([`code_of`]), which every test's engine compiles it into, so a program is compiled once for
//! each build of it rather than once for each test that runs it. Beside them, `INCOMPATIBLE`,
//! `BUILT_AGAINST_2_0` and `IMPORTS_INSPECT`, components written by hand, which no build makes.
//!
//! The invariants: a program is built by the build script alone, into this crate's own target
//! directory, so a test never builds one and never waits on the workspace's build; and a code
//! folder is named by its component's content, so a program built again is compiled again, as the
//! engine's contract asks: a code folder belongs to one component that never changes.

use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/programs.rs"));

/// A component built for another version of the interface, which the engine cannot link: it
/// imports `wasdk:platform/system` with an `operating-system` of one case less. Text, which the
/// engine compiles as it does a component's binary.
pub const INCOMPATIBLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/incompatible.wat");

/// A component built against `wasdk:platform@2.0.0`, the version before `service-extensions`,
/// which the engine, serving `2.2.0`, links by semver's rule: it imports `wasdk:platform/system`
/// by that version and nothing else. Text, which the engine compiles as it does a component's
/// binary.
pub const BUILT_AGAINST_2_0: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/built-against-2-0.wat");

/// A component that asks for the inspector role: it imports an interface of `wasdk:inspect`, and
/// nothing else. Text, which the engine compiles as it does a component's binary.
pub const IMPORTS_INSPECT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/imports-inspect.wat");

/// The code folder of `component`, one of the components above, named by its content.
///
/// # Panics
///
/// When `component` is none of them.
pub fn code_of(component: &str) -> PathBuf {
    code_folder(component).unwrap_or_else(|| panic!("{component} is none of the test programs"))
}

/// The code folder of `component` where it is one of the components above; none otherwise.
pub fn code_folder(component: &str) -> Option<PathBuf> {
    CODE.iter()
        .find(|(listed, _)| *listed == component)
        .map(|(_, folder)| PathBuf::from(folder))
}
