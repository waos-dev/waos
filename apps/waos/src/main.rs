//! The `waos` binary: the door to WaOS, which is the library beside it. On Android the activity
//! loads the library itself, and the binary has no entry. On Windows it is a GUI program, which
//! opens no console window; its entry sends what it writes to the terminal it was run from.

#![cfg_attr(target_os = "android", no_main)]
#![windows_subsystem = "windows"]

#[cfg(not(target_os = "android"))]
fn main() {
    waos_app::main();
}
