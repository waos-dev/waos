//! A program's threads: the component model's cooperative threads, which the engine takes, so a
//! program linked with a libc that has them runs its `std::thread`s. A program built with one
//! needs wasi-sdk's cooperative-threads sysroot, which CI does not install, so this loads a
//! component that calls the threads' built-ins directly; the experiment
//! `2026-11-22-std-threads-on-wasip3` ran a wasdk program's `std::thread`s in the engine.

use waos::WaOS;

use crate::test_shell::{TestShell, config};

/// A component whose one function yields its thread and answers the thread's index, through the
/// built-ins wasmtime refuses unless the component model's threading is on.
const YIELDS_ITS_THREAD: &str = r#"
(component
  (core func $index (canon thread.index))
  (core func $yield (canon thread.yield))
  (core module $m
    (import "" "index" (func $index (result i32)))
    (import "" "yield" (func $yield (result i32)))
    (func (export "f") (result i32) call $yield drop call $index))
  (core instance $i (instantiate $m (with "" (instance (export "index" (func $index)) (export "yield" (func $yield))))))
  (func (export "f") (result u32) (canon lift (core func $i "f"))))
"#;

#[test]
fn a_component_using_cooperative_threads_loads() {
    let folder = tempfile::tempdir().unwrap();
    let component = folder.path().join("yields-its-thread.wat");
    std::fs::write(&component, YIELDS_ITS_THREAD).unwrap();
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    engine
        .load(&component)
        .unwrap_or_else(|error| panic!("the threads' built-ins are refused: {error:#}"));
}
