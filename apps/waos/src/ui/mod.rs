//! The app's widgets: every view at the root, each built from what it is (`every_window`), the
//! prompts' views in dialogs of the programs' apps on macOS among them; the library, in the layout
//! its window's width takes, and on a desktop its launcher's window (`launcher`); on a phone the deck, the library and the
//! programs' tabs; the programs' windows, `waos-program-window`'s, with the prompts WaOS places
//! over them or in windows of their own; and the question sheet, the prompt WaOS asks a program's
//! user through.
//!
//! The invariant: a widget reads the state and tells the program what happens in it; it decides
//! nothing about where views go, which is `state`'s.

pub(crate) mod deck;
pub(crate) mod every_window;
#[cfg(test)]
mod gallery;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod launcher;
pub(crate) mod library;
pub(crate) mod question_sheet;
