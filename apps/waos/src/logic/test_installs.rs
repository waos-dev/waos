//! Installing a test program as the package manager installs one, without compiling it again: the
//! version is staged, then its code folder is filled with links to the code of the test program it
//! is a copy of, compiled once per test run into that program's code folder
//! (`waos_test_programs::code_of`), then committed. A compile is a function of the component's
//! bytes and the engine alone, so the code is the code an install would compile; and a code file
//! is never changed in place, so a link to it is safe.
//!
//! The invariant: a test that installs a program it then opens or launches installs it here, so
//! no test waits on a compile it did not ask for; a test of the install itself goes through the
//! package manager, and compiles.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use waos::WaOS;
use waos_package::Package;
use waos_store::{DevelopmentBuild, InstalledProgram, InstalledVersion, Staged, Store};

/// The smallest component, as text, which the engine compiles as it does a binary: what a test
/// installs where it shows or removes a program but never runs it, since an install compiles, and
/// this compiles at once.
pub(crate) const EMPTY_COMPONENT: &[u8] = b"(component)";

/// Installs `package`, whose component is a copy of the test program at `component`, into
/// `store`, with its code compiled by an engine like `engine`.
pub(crate) fn install_package(
    store: &Store,
    engine: &WaOS,
    package: Package,
    component: &str,
) -> InstalledProgram {
    let staged = store.stage(package).expect("the package stages");
    commit_compiled(store, engine, staged, component)
}

/// Installs `build`, whose component is a copy of the test program at `component`, into `store`
/// as its development install, with its code compiled by an engine like `engine`.
pub(crate) fn install_development(
    store: &Store,
    engine: &WaOS,
    build: &DevelopmentBuild<'_>,
    component: &str,
) -> InstalledProgram {
    let staged = store.stage_development(build).expect("the build stages");
    commit_compiled(store, engine, staged, component)
}

/// Commits `staged`, its code folder filled with the code of the test program at `component`,
/// compiled first where this run has not compiled it yet.
fn commit_compiled(
    store: &Store,
    engine: &WaOS,
    staged: Staged,
    component: &str,
) -> InstalledProgram {
    let compiled = compiled(engine, component);
    let code = staged.version().code();
    fs::create_dir_all(&code).unwrap();
    for file in fs::read_dir(&compiled).unwrap() {
        let file = file.unwrap().path();
        let linked = code.join(file.file_name().unwrap());
        if fs::hard_link(&file, &linked).is_err() {
            fs::copy(&file, &linked).unwrap();
        }
    }
    store.commit(staged).expect("the program installs")
}

/// The code folder of the test program at `component`, which `engine` has compiled it into: one
/// test compiles it while the others that need it wait.
fn compiled(engine: &WaOS, component: &str) -> PathBuf {
    static COMPILING: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let code = waos_test_programs::code_of(component);
    let turn = {
        let mut compiling = COMPILING.get_or_init(Mutex::default).lock().unwrap();
        Arc::clone(compiling.entry(code.clone()).or_default())
    };
    let _turn = turn.lock().unwrap();
    if !engine.is_compiled(&code) {
        engine
            .compile(Path::new(component), &code)
            .expect("the test program compiles");
    }
    code
}

/// Spoils `version`, installed, as a disk that lost its files would: its component and every code
/// file in its code folder, so the engine neither maps the code nor compiles it again.
pub(crate) fn spoil(version: &InstalledVersion) {
    fs::write(version.component(), b"spoiled").unwrap();
    for file in fs::read_dir(version.code()).unwrap() {
        fs::write(file.unwrap().path(), b"spoiled").unwrap();
    }
}
