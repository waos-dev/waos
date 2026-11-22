//! A program's simulated user: what a tool does to a program's views as its user would, through
//! the engine's views as the shell reports a user's input, so the program cannot tell the two
//! apart (the design's decision 64): a mouse of the program's own (`mouse`), a US keyboard's keys
//! and chords (`keyboard`, its keys' numbers in `key_codes`), and text committed as an input
//! method commits it (`text_commit`). What drives a program, `waos tap` and a test among them,
//! goes through here, so each gives the program the same events.
//!
//! The invariant a later editor will break: nothing here goes past the engine's view. The system
//! and the shell never see these events, so a key the program leaves goes no further, where a
//! real one would go on to the window's menu bar.

mod key_codes;
mod keyboard;
mod mouse;
mod text_commit;

use std::time::Duration;

use crate::views::{ProgramView, ProgramViews};

pub use mouse::DragPath;
use mouse::Mouse;

/// A program's simulated user, which every view of the program shares: its mouse is in one of
/// them at a time.
pub struct SimulatedUser {
    views: ProgramViews,
    mouse: Mouse,
}

/// A chord whose names name no key, or whose keys before the last are not modifiers.
#[derive(Debug)]
pub struct NoSuchChord;

/// Text to commit while nothing in the view is edited.
#[derive(Debug)]
pub struct NothingEdited;

impl SimulatedUser {
    /// The pointer device the simulated mouse is: a number no host gives a pointer, the mouse a
    /// window reports being 0, as Flutter's `kMousePointerDeviceId` is.
    pub const MOUSE_DEVICE: i64 = mouse::DEVICE;

    /// The user of the program whose views are `views`.
    pub(crate) fn new(views: ProgramViews) -> SimulatedUser {
        SimulatedUser {
            views,
            mouse: Mouse::default(),
        }
    }

    /// A press and release of the mouse's primary button at `at` in `view`, in its physical
    /// pixels, the mouse coming there first.
    pub fn tap(&self, view: &ProgramView, at: (f64, f64)) {
        self.mouse.tap(&self.views, view, at);
    }

    /// A press of the primary button at `at` in `view`, the mouse coming there first, held for
    /// `held`, then released.
    pub async fn press(&self, view: &ProgramView, at: (f64, f64), held: Duration) {
        self.mouse.press(&self.views, view, at, held).await;
    }

    /// A drag of the primary button along `path` in `view`: pressed at its start, held, moved
    /// along it as its time comes, and released at its end.
    pub async fn drag(&self, view: &ProgramView, path: &DragPath) {
        self.mouse.drag(&self.views, view, path).await;
    }

    /// A mouse wheel's scroll by `by` pixels at `at` in `view`, a positive `by.1` scrolling
    /// towards the content's end, the mouse coming there first.
    pub fn scroll(&self, view: &ProgramView, at: (f64, f64), by: (f64, f64)) {
        self.mouse.scroll(&self.views, view, at, by);
    }

    /// The key or the chord `chord` names (`enter`, `a`, `cmd+s`) pressed in `view`, as a US
    /// keyboard sends it. The program's replies are not awaited: a key it leaves goes no further.
    pub fn key(&self, view: &ProgramView, chord: &str) -> Result<(), NoSuchChord> {
        let events = keyboard::key_events(chord, view.now()).ok_or(NoSuchChord)?;
        for event in events {
            drop(view.key(event));
        }
        Ok(())
    }

    /// `text` committed into what `view` edits, as an input method commits its text, in place of
    /// what is composed or selected.
    pub fn type_text(&self, view: &ProgramView, text: &str) -> Result<(), NothingEdited> {
        let editing = view.editing_value().ok_or(NothingEdited)?;
        view.text_input_editing_value(text_commit::committed(&editing, text));
        Ok(())
    }
}
