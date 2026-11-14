//! WaOS's window library: a program's window process on macOS, an Waft app of its own holding
//! one program's windows, which each program's app bundle loads with its loader
//! (`bin/waos-program-loader`). The engine stays in WaOS; the window process shows the program's
//! windows (`waos-program-window`'s), links them to the program through WaOS over Mach
//! (`program_link`, `connection`), each window waiting in its resize for the program's picture
//! at the new size (`resize_waits`), shows its menu bar and opens its context menus
//! (`program_menus`) and its file dialogs (`program_file_dialogs`), posts its notifications under
//! its bundle (`program_notifications`), shows WaOS's prompts for the program in dialog windows of
//! their own, drawn in WaOS (`prompt_dialogs`), and says hello to the WaOS it finds or starts
//! (`finding_waos`, `window_process`). What it asks of the system itself
//! is `os`'s.
//!
//! The invariant: the window process is a program's windows and nothing of WaOS's own. It runs no
//! program's code and decides nothing a program's user is asked; it carries what the user does to
//! WaOS and what the program asks back.

#![cfg(target_os = "macos")]
// Waft's states take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]
// Proving that a type holding a program's handle crosses threads walks into wgpu-core's
// registries, deeper than the default limit allows (the engine's README).
#![recursion_limit = "256"]

mod connection;
mod dialog_frame;
mod finding_waos;
mod hosted_layers;
mod os;
mod program_file_dialogs;
mod program_link;
mod program_menus;
mod program_notifications;
mod program_quit;
mod prompt_canvas;
mod prompt_dialogs;
mod prompt_pointers;
mod resize_waits;
mod window_process;
