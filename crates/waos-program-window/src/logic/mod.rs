//! Plain Rust, tested without Waft's UI: what goes between Waft's terms and the program's (a
//! window's configuration, its title bar, geometry, how a view stands, pointers, keys, the cursor, text input,
//! where a layer is placed, the style of the system's bars, a menu, a file dialog's options and picks, a notification and a click
//! on one), what a program's window carries for the system to restore it with, the calls only a window's main thread can carry out, the composition layer a canvas
//! shows, the program's own layers on Waft's compositor, and the shell's side of a view whose
//! program runs in this process.
//!
//! The invariant: nothing here names Waft's application or a widget. What ties this to the
//! application is `state`'s.

pub(crate) mod canvas_layer;
pub(crate) mod canvas_view;
pub(crate) mod composition;
pub(crate) mod cursor;
pub(crate) mod file_dialogs;
pub(crate) mod geometry;
pub(crate) mod keys;
pub(crate) mod menu_entries;
pub(crate) mod notifications;
pub(crate) mod overlay_style;
pub(crate) mod placements;
pub(crate) mod pointer;
pub(crate) mod presence;
pub(crate) mod restoration;
pub(crate) mod text_input;
pub(crate) mod title_bar;
pub(crate) mod view_calls;
pub(crate) mod window_config;
