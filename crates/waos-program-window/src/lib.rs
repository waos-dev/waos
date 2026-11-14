//! A program's windows as its user meets them, on any desktop: each view a program asks for in an
//! Waft window of its own, its canvas filling it, the pointer, the keys and the keyboard's focus
//! going to the program, its text open to the input methods, and Edit's commands sent to it.
//!
//! What WaOS asks a program's user is a prompt, one widget on every platform, shown attached to the
//! program's window by a [`PromptPlacement`]: in WaOS's own windows, by [`PromptWindows`].
//!
//! The windows talk to their program through [`ProgramLink`] and [`ViewLink`], whichever process
//! the program runs in: WaOS's own windows reach a program in WaOS's process through the engine's
//! own types (`in_process`), and a program's window process reaches it through messages to WaOS.
//! What the windows carry crosses as the values the engine's shell traits already take: the
//! program's calls on a view are [`ViewCall`]s, and what a window reports are the program's own
//! records, as wasmtime generates them from the WIT.
//!
//! The code is in Waft's layers, as WaOS's is: `logic` is plain Rust, tested without Waft's UI,
//! the conversions between Waft's terms and the program's and the shell's side of a view in
//! WaOS's process; `state` ties it to the application, the windows and each view shown; `ui` holds
//! the widgets, the windows, each view's canvas, its text input and Edit's commands.
//!
//! The invariant: nothing here waits on the program, but for a resize its link holds until the
//! program's picture at the new size ([`ViewLink::metrics_changed`]), and nothing here decides what
//! the process the windows are in shows over them, which it hands in ([`OverWindow`]).

// Waft's observers and states take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]
// Proving that a type holding a program's handle crosses threads walks into wgpu-core's
// registries, deeper than the default limit allows (the engine's README).
#![recursion_limit = "256"]

mod in_process;
mod link;
mod logic;
mod prompt_placement;
mod state;
mod ui;

pub use in_process::placed_view;
pub use link::{OpenedView, ProgramFace, ProgramIdentity, ProgramLink, ViewLink, WindowRequest};
pub use logic::canvas_layer::CanvasLayer;
pub use logic::canvas_view::{Canvas, CanvasView};
pub use logic::composition::{WaftCompositor, waft_layer};
pub use logic::file_dialogs::{
    PickedPaths, picked_file_paths, picked_folder_path, waft_folder_options, waft_open_options,
    waft_save_options,
};
pub use logic::geometry::{metrics_of, rect_of};
pub use logic::menu_entries::{waft_image, waft_menu};
pub use logic::notifications::{activation_of, waft_notification};
pub use logic::restoration::{RestorableWindow, WindowRole};
pub use logic::view_calls::{ViewCalls, WindowCalls};
pub use prompt_placement::{PROMPT_SIZE, PromptPlacement, ShownPrompt};
pub use state::prompt_windows::{PROMPT_WINDOW, PromptWindows};
pub use state::shown_view::ShownView;
pub use state::view_windows::ViewWindows;
pub use ui::program_canvas::ProgramCanvas;
pub use ui::program_picture::ProgramPicture;
pub use ui::program_windows::{OverWindow, ProgramWindows};
pub use ui::window_changes::WindowChanges;
