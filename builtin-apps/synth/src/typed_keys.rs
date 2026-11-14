//! The computer's keys played: a key of the piano row holds its note from the moment it goes
//! down until it comes up, the system's repeats of a held key ignored, and Z and X move the
//! keyboard an octave. Keys pressed with Command, Control or Alt are left to their shortcuts.
//!
//! The keys are heard from the keyboard itself, not through the focus: a note must not depend
//! on which control was clicked last, or on a list being open.

use std::rc::Rc;

use waft::App;
use waft_services::{HardwareKeyboard, KeyEvent, PhysicalKeyboardKey};

use crate::held_keys::Holder;
use crate::instrument::Instrument;
use crate::typing::{self, TypedKey};

/// Plays the computer's keys on `instrument` from now on, whatever has the focus.
pub(crate) fn play_typed_keys(instrument: &Instrument, app: &mut App) {
    let instrument = instrument.clone();
    let keyboard = HardwareKeyboard::instance(app);
    keyboard.add_handler(
        app,
        Rc::new(move |app, event| played(&instrument, app, event)),
    );
}

/// Plays `event` on `instrument`. Answers whether the key was one of the instrument's.
fn played(instrument: &Instrument, app: &mut App, event: &KeyEvent) -> bool {
    let key = event.physical_key();
    let holder = Holder::ComputerKey(key.usb_hid_usage);
    match event {
        KeyEvent::Up(_) => {
            let held = instrument.is_held_by(app, holder);
            instrument.hold(app, holder, None);
            held
        }
        KeyEvent::Repeat(_) => instrument.is_held_by(app, holder),
        KeyEvent::Down(_) => !is_a_shortcut(app) && pressed(instrument, app, key, holder),
    }
}

/// A key went down: plays its note, or moves the keyboard. Answers whether it did either.
fn pressed(
    instrument: &Instrument,
    app: &mut App,
    key: PhysicalKeyboardKey,
    holder: Holder,
) -> bool {
    match typing::typed(key) {
        Some(TypedKey::Semitone(semitone)) => {
            let note = instrument.playing().read(app).note_above_lowest(semitone);
            instrument.hold(app, holder, note);
            true
        }
        Some(TypedKey::Octaves(octaves)) => {
            instrument.shift_octaves(app, octaves);
            true
        }
        None => false,
    }
}

/// Whether a key that makes a shortcut of the next is held.
fn is_a_shortcut(app: &mut App) -> bool {
    let keyboard = HardwareKeyboard::instance(app);
    keyboard.is_meta_pressed(app)
        || keyboard.is_control_pressed(app)
        || keyboard.is_alt_pressed(app)
}
