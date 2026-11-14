//! The window library's entry, which a program's loader finds by name and calls
//! (`waos_window_messages::bundle::WINDOW_MAIN`).

#![allow(unsafe_code)]

use std::ffi::c_int;

/// Runs the window process until it ends; answers its exit status.
#[unsafe(no_mangle)]
pub extern "C" fn waos_window_main_v1() -> c_int {
    crate::window_process::run()
}

#[cfg(test)]
mod tests {
    use waos_window_messages::bundle::WINDOW_MAIN;

    /// The loader looks the entry up by the name the bundle's contract gives it.
    #[test]
    fn the_entry_is_exported_under_the_contracts_name() {
        assert_eq!(WINDOW_MAIN, "waos_window_main_v1");
    }
}
