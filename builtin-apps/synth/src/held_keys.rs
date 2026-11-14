//! Which notes are held, and by what: a finger or the mouse on the keyboard drawn, or a key of
//! the computer's. A note sounds while anything holds it, and, while the instrument sustains,
//! after it was let go, until the instrument sustains no more.

use std::collections::HashMap;

use crate::pitch::Note;

/// What holds a note down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Holder {
    /// A pointer, by the number its events carry: the mouse, or one finger of several.
    Pointer(i64),
    /// A key of the computer's keyboard, by its USB usage.
    ComputerKey(u64),
}

/// What the sound is asked for as a holder moves: a note let go, a note pressed, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) let_go: Option<Note>,
    pub(crate) press: Option<Note>,
}

#[derive(Default)]
pub(crate) struct HeldKeys {
    notes: HashMap<Holder, Note>,
    /// Whether a note let go is kept sounding, and the notes kept so.
    sustaining: bool,
    sustained: Vec<Note>,
}

impl HeldKeys {
    /// `holder` is on `note` now, or on none. Its note before is let go unless another still
    /// holds it or the instrument sustains, and its new note pressed, again if it sounds
    /// already.
    pub(crate) fn put(&mut self, holder: Holder, note: Option<Note>) -> Change {
        let before = match note {
            Some(note) => self.notes.insert(holder, note),
            None => self.notes.remove(&holder),
        };
        if before == note {
            return Change::default();
        }
        if let Some(note) = note {
            self.sustained.retain(|sustained| *sustained != note);
        }
        Change {
            let_go: before.filter(|before| !self.is_held(*before) && !self.sustain(*before)),
            press: note,
        }
    }

    /// No holder holds a note any more. Answers the notes to let go.
    pub(crate) fn drop_holders(&mut self) -> Vec<Note> {
        let mut held: Vec<Note> = self.notes.drain().map(|(_, note)| note).collect();
        held.sort();
        held.dedup();
        held.retain(|note| !self.sustain(*note));
        held
    }

    pub(crate) fn is_sustaining(&self) -> bool {
        self.sustaining
    }

    /// Sustains from now on, or no more. Answers the notes to let go: the sustained ones
    /// nothing holds, once it sustains no more.
    pub(crate) fn set_sustaining(&mut self, sustaining: bool) -> Vec<Note> {
        self.sustaining = sustaining;
        if sustaining {
            return Vec::new();
        }
        let sustained = std::mem::take(&mut self.sustained);
        sustained
            .into_iter()
            .filter(|note| !self.is_held(*note))
            .collect()
    }

    /// The notes that sound, in no order.
    pub(crate) fn notes(&self) -> Vec<Note> {
        self.notes
            .values()
            .chain(&self.sustained)
            .copied()
            .collect()
    }

    /// Whether `holder` holds a note.
    pub(crate) fn holds(&self, holder: Holder) -> bool {
        self.notes.contains_key(&holder)
    }

    fn is_held(&self, note: Note) -> bool {
        self.notes.values().any(|held| *held == note)
    }

    /// Keeps `note`, just let go, sounding where the instrument sustains. Answers whether it
    /// does.
    fn sustain(&mut self, note: Note) -> bool {
        if self.sustaining && !self.sustained.contains(&note) {
            self.sustained.push(note);
        }
        self.sustaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINGER: Holder = Holder::Pointer(1);
    const OTHER_FINGER: Holder = Holder::Pointer(2);
    const KEY: Holder = Holder::ComputerKey(4);

    #[test]
    fn a_holder_presses_its_note_and_lets_it_go() {
        let mut held = HeldKeys::default();
        let pressed = held.put(FINGER, Some(Note(60)));
        assert_eq!(
            pressed,
            Change {
                let_go: None,
                press: Some(Note(60))
            }
        );
        assert!(held.notes().contains(&Note(60)));
        let let_go = held.put(FINGER, None);
        assert_eq!(
            let_go,
            Change {
                let_go: Some(Note(60)),
                press: None
            }
        );
        assert!(!held.notes().contains(&Note(60)));
    }

    #[test]
    fn a_finger_slid_to_the_next_key_lets_one_note_go_and_presses_the_other() {
        let mut held = HeldKeys::default();
        held.put(FINGER, Some(Note(60)));
        let slid = held.put(FINGER, Some(Note(62)));
        assert_eq!(
            slid,
            Change {
                let_go: Some(Note(60)),
                press: Some(Note(62))
            }
        );
        assert_eq!(
            held.put(FINGER, Some(Note(62))),
            Change::default(),
            "still on it"
        );
    }

    #[test]
    fn a_note_two_hold_sounds_until_both_let_go() {
        let mut held = HeldKeys::default();
        held.put(FINGER, Some(Note(60)));
        let again = held.put(KEY, Some(Note(60)));
        assert_eq!(again.press, Some(Note(60)), "pressed again");
        assert_eq!(held.put(FINGER, None), Change::default());
        assert!(held.notes().contains(&Note(60)));
        assert_eq!(held.put(KEY, None).let_go, Some(Note(60)));
    }

    #[test]
    fn several_fingers_hold_a_note_each() {
        let mut held = HeldKeys::default();
        held.put(FINGER, Some(Note(60)));
        held.put(OTHER_FINGER, Some(Note(64)));
        assert!(held.notes().contains(&Note(60)) && held.notes().contains(&Note(64)));
        assert!(held.holds(OTHER_FINGER));
        assert_eq!(held.drop_holders(), [Note(60), Note(64)]);
        assert!(!held.notes().contains(&Note(60)) && !held.holds(FINGER));
        assert!(held.drop_holders().is_empty());
    }

    #[test]
    fn a_note_let_go_while_sustaining_sounds_until_sustaining_ends() {
        let mut held = HeldKeys::default();
        assert!(held.set_sustaining(true).is_empty());
        held.put(FINGER, Some(Note(60)));
        assert_eq!(held.put(FINGER, None), Change::default(), "kept sounding");
        assert!(held.notes().contains(&Note(60)));
        held.put(FINGER, Some(Note(64)));
        assert!(
            held.drop_holders().is_empty(),
            "as are the notes whose holders are dropped"
        );
        held.put(KEY, Some(Note(67)));
        assert_eq!(
            held.set_sustaining(false),
            [Note(60), Note(64)],
            "but not a note still held"
        );
        assert!(held.notes().contains(&Note(67)) && !held.notes().contains(&Note(60)));
    }

    #[test]
    fn a_sustained_note_pressed_again_is_played_again_and_held() {
        let mut held = HeldKeys::default();
        held.set_sustaining(true);
        held.put(FINGER, Some(Note(60)));
        held.put(FINGER, None);
        assert_eq!(held.put(KEY, Some(Note(60))).press, Some(Note(60)));
        assert_eq!(held.notes(), [Note(60)], "once, not twice");
        assert!(
            held.set_sustaining(false).is_empty(),
            "the key still holds it"
        );
    }
}
