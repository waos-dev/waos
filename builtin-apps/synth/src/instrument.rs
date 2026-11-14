//! The instrument as its user plays it: the preset chosen, where its controls stand, where the
//! keyboard starts and which keys are held, over the sound that plays them.
//!
//! The invariant: the sound is asked for exactly what the held keys change to, so a key that
//! went down is always let go: by its holder, by the app losing its user's attention, or, where
//! the instrument sustains, by its user ending that.

use std::cell::RefCell;
use std::rc::Rc;

use waft::{App, Entity};

use crate::control::Control;
use crate::held_keys::{Change, HeldKeys, Holder};
use crate::output::{Command, Output};
use crate::pitch::{Note, OCTAVE};
use crate::presets::Preset;
use crate::tone::Tone;
use crate::trace::Trace;

/// The octaves the keyboard's lowest note is kept in, and the one it starts in.
const LOWEST_OCTAVE: u8 = 1;
const HIGHEST_OCTAVE: u8 = 6;
const FIRST_OCTAVE: u8 = 3;

/// What the widgets read and rebuild from.
pub(crate) struct Playing {
    pub(crate) preset: Preset,
    pub(crate) tone: Tone,
    /// The keyboard's lowest note, a C.
    pub(crate) lowest: Note,
    pub(crate) held: HeldKeys,
}

#[derive(Clone)]
pub(crate) struct Instrument {
    playing: Entity<Playing>,
    /// The sound; none where the platform has none.
    output: Option<Output>,
}

impl Instrument {
    /// The instrument on its first preset, no key held, playing on the platform's output where
    /// it has one.
    pub(crate) fn new(app: &mut App) -> Instrument {
        let preset = Preset::ALL[0];
        let tone = preset.tone();
        let playing = app.new_entity(|_cx| Playing {
            preset,
            tone,
            lowest: Note::c(FIRST_OCTAVE),
            held: HeldKeys::default(),
        });
        Instrument {
            playing,
            output: Output::start(preset, tone),
        }
    }

    pub(crate) fn playing(&self) -> &Entity<Playing> {
        &self.playing
    }

    pub(crate) fn has_sound(&self) -> bool {
        self.output.is_some()
    }

    /// What was played last, for the scope, where there is sound.
    pub(crate) fn trace(&self) -> Option<&Rc<RefCell<Trace>>> {
        self.output.as_ref().map(Output::trace)
    }

    /// `holder` is on `note` now, or on none. The sound is asked before the widgets hear of
    /// it, so the scope finds the sound's trace live once a key is pressed.
    pub(crate) fn hold(&self, app: &mut App, holder: Holder, note: Option<Note>) {
        let change = self
            .playing
            .update(app, |playing, _cx| playing.held.put(holder, note));
        if change == Change::default() {
            return;
        }
        if let Some(note) = change.let_go {
            self.ask(Command::LetGo(note));
        }
        if let Some(note) = change.press {
            self.ask(Command::Press(note));
        }
        self.playing.update(app, |_playing, cx| cx.notify());
    }

    /// Whether `holder` holds a note.
    pub(crate) fn is_held_by(&self, app: &App, holder: Holder) -> bool {
        self.playing.read(app).held.holds(holder)
    }

    /// Nothing holds a key any more: every note not sustained is let go.
    pub(crate) fn drop_holders(&self, app: &mut App) {
        let let_go = self
            .playing
            .update(app, |playing, _cx| playing.held.drop_holders());
        self.let_go(app, let_go);
    }

    /// Keeps notes sounding after their keys are let go, or lets them go from now on, and the
    /// notes kept so far with it.
    pub(crate) fn sustain(&self, app: &mut App, sustaining: bool) {
        let let_go = self
            .playing
            .update(app, |playing, _cx| playing.held.set_sustaining(sustaining));
        self.let_go(app, let_go);
    }

    /// Lets `notes` go, and has the widgets draw the keys anew.
    fn let_go(&self, app: &mut App, notes: Vec<Note>) {
        for note in notes {
            self.ask(Command::LetGo(note));
        }
        self.playing.update(app, |_playing, cx| cx.notify());
    }

    /// Plays `preset` from the next key on, its controls where the preset starts them.
    pub(crate) fn choose_preset(&self, app: &mut App, preset: Preset) {
        let tone = preset.tone();
        self.playing.update(app, |playing, cx| {
            playing.preset = preset;
            playing.tone = tone;
            cx.notify();
        });
        self.ask(Command::ChangePreset(preset, tone));
    }

    /// Moves `control` to `value`.
    pub(crate) fn move_control(&self, app: &mut App, control: Control, value: f32) {
        let tone = self.playing.update(app, |playing, cx| {
            playing.tone = control.set(playing.tone, value);
            cx.notify();
            playing.tone
        });
        self.ask(Command::SetTone(tone));
    }

    /// Moves the keyboard by `octaves`, as far as its range goes. Keys held stay on their notes.
    pub(crate) fn shift_octaves(&self, app: &mut App, octaves: i8) {
        self.playing.update(app, |playing, cx| {
            let octave = playing.lowest.octave().saturating_add_signed(octaves);
            let lowest = Note::c(octave.clamp(LOWEST_OCTAVE, HIGHEST_OCTAVE));
            if lowest != playing.lowest {
                playing.lowest = lowest;
                cx.notify();
            }
        });
    }

    fn ask(&self, command: Command) {
        if let Some(output) = &self.output {
            output.ask(command);
        }
    }
}

impl Playing {
    /// Whether the keyboard can move by `octaves`.
    pub(crate) fn can_shift_octaves(&self, octaves: i8) -> bool {
        let octave = self.lowest.octave().saturating_add_signed(octaves);
        (LOWEST_OCTAVE..=HIGHEST_OCTAVE).contains(&octave)
    }

    /// The note `semitones` above the keyboard's lowest.
    pub(crate) fn note_above_lowest(&self, semitones: u8) -> Option<Note> {
        debug_assert!(semitones < 2 * OCTAVE);
        self.lowest.up(semitones)
    }
}
