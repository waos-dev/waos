//! The widgets: the programs' windows, each with its view's canvas, the canvas's text input and
//! Edit's commands, and what the process shows over it; a view's last picture alone, with no
//! input; WaOS's prompts in its own windows; and the reporter of a window's changes to state that
//! follows it.
//!
//! The invariant: a widget reads the state and tells the program what happens in it; it decides
//! nothing about where views go, which is `state`'s.

pub(crate) mod edit_commands;
pub(crate) mod program_canvas;
pub(crate) mod program_picture;
pub(crate) mod program_text_input;
pub(crate) mod program_windows;
pub(crate) mod prompt_windows;
pub(crate) mod window_changes;
