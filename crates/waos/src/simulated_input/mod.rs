//! Input a tool gives a program's views as its user would, through the engine's views as the shell
//! reports a user's input, so the program cannot tell the two apart: a mouse of the program's own
//! (`mouse`), which remembers which of its views it is in, and a US keyboard's keys and chords and
//! text inserted as an input method commits it (`keyboard`, its keys' numbers in `key_codes`, the
//! commit in `text_commit`). What drives a program, `waos tap` and a test among them, goes through
//! here, so each gives the program the same events.
//!
//! The invariant a later editor will break: nothing here goes past the engine's view. The system
//! and the shell never see these events, so a key the program leaves goes no further, where a
//! real one would go on to the window's menu bar.

mod key_codes;
mod keyboard;
mod mouse;
mod text_commit;

pub use keyboard::{Keyboard, NoSuchChord, NothingEdited};
pub use mouse::{DragPath, Mouse};
