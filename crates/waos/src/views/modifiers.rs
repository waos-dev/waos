//! The modifier keys of a program's keyboard: those the keyboard holds, as the shell reports them
//! with a pointer event ([`HeldModifiers`]), and those the program has been told are held
//! ([`ToldModifiers`]), caught up with the keyboard before each pointer event, as Flutter's
//! desktop embedders do (flutter/engine#37870).
//!
//! A program hears keys only while one of its views has the keyboard's focus, so a Shift pressed
//! or released while another window had it never reaches the program. Before a pointer event,
//! each modifier key whose state differs from the keyboard's arrives as a synthesized key event,
//! so a shift-click reads as one. A program has one keyboard for all its views, so there is one
//! record per program, which its views share. The invariant: every key event the program hears
//! through any of its views, told or synthesized, passes through [`ToldModifiers::record`].

use std::collections::HashMap;

use crate::bindings::wasdk::platform::views::{KeyDevice, KeyEvent, KeyKind};

/// The keys kept in step: Control, Shift, Alt and Meta, left and right, by their physical keys'
/// USB HID usages, as a key event names them.
const MODIFIER_KEYS: [u64; 8] = [
    0x0007_00e0, // Control, left
    0x0007_00e1, // Shift, left
    0x0007_00e2, // Alt, left
    0x0007_00e3, // Meta, left
    0x0007_00e4, // Control, right
    0x0007_00e5, // Shift, right
    0x0007_00e6, // Alt, right
    0x0007_00e7, // Meta, right
];

/// The modifier keys the keyboard holds: each one's physical key, with the logical key it means
/// on the keyboard's layout now, named as a [`KeyEvent`] names them.
///
/// Collected from the keys the keyboard holds, each a pair of its physical key and its logical
/// key, as a map is collected from its keys and values: it keeps the modifier keys among them,
/// since which keys those are is the engine's to know. With no keyboard, it is the default:
/// nothing held.
#[derive(Clone, Debug, Default)]
pub struct HeldModifiers(HashMap<u64, u64>);

impl HeldModifiers {
    /// Each modifier key held: its physical key, with the logical key it means. What the shell
    /// sends on when the view is shown in another process, which collects them back.
    pub fn keys(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.0
            .iter()
            .map(|(physical, logical)| (*physical, *logical))
    }
}

impl FromIterator<(u64, u64)> for HeldModifiers {
    fn from_iter<I: IntoIterator<Item = (u64, u64)>>(keys: I) -> HeldModifiers {
        let modifiers = keys
            .into_iter()
            .filter(|(physical, _)| MODIFIER_KEYS.contains(physical))
            .collect();
        HeldModifiers(modifiers)
    }
}

/// The modifier keys a program has been told are held: each one's physical key, with the
/// logical key the program was told it means.
#[derive(Default)]
pub(super) struct ToldModifiers(HashMap<u64, u64>);

impl ToldModifiers {
    /// Keeps up with a key event the program is told of.
    pub(super) fn record(&mut self, event: &KeyEvent) {
        if !MODIFIER_KEYS.contains(&event.physical) {
            return;
        }
        match event.kind {
            KeyKind::Down | KeyKind::Repeat => {
                self.0.insert(event.physical, event.logical);
            }
            KeyKind::Up => {
                self.0.remove(&event.physical);
            }
        }
    }

    /// The synthesized key events that bring the program to the modifier keys `held`, stamped
    /// `time`. The program is taken to have been told them.
    pub(super) fn catch_up(&mut self, held: &HeldModifiers, time: u64) -> Vec<KeyEvent> {
        let corrections: Vec<KeyEvent> = MODIFIER_KEYS
            .into_iter()
            .filter_map(|physical| self.correction(physical, held, time))
            .collect();
        for correction in &corrections {
            self.record(correction);
        }
        corrections
    }

    /// The key event that tells the program what `physical` is doing on the keyboard, when it
    /// was told otherwise. A release carries the logical key the program was told went down.
    fn correction(&self, physical: u64, held: &HeldModifiers, time: u64) -> Option<KeyEvent> {
        match (self.0.get(&physical), held.0.get(&physical)) {
            (None, Some(&logical)) => Some(synthesized(KeyKind::Down, physical, logical, time)),
            (Some(&logical), None) => Some(synthesized(KeyKind::Up, physical, logical, time)),
            _ => None,
        }
    }
}

/// A key event made up to keep the program's picture of the keyboard right: it types nothing.
fn synthesized(kind: KeyKind, physical: u64, logical: u64, time: u64) -> KeyEvent {
    KeyEvent {
        time,
        kind,
        device: KeyDevice::Keyboard,
        physical,
        logical,
        character: None,
        synthesized: true,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Keys as a key event names them: the physical key's USB HID usage, and Flutter's id for
    /// the logical key.
    pub(in crate::views) const SHIFT: (u64, u64) = (0x0007_00e1, 0x0002_0000_0102);
    const META: (u64, u64) = (0x0007_00e7, 0x0002_0000_0107);
    const KEY_A: (u64, u64) = (0x0007_0004, 0x61);

    /// A key event the shell reports, the way the platform saw it.
    pub(in crate::views) fn key_event(kind: KeyKind, (physical, logical): (u64, u64)) -> KeyEvent {
        KeyEvent {
            time: 0,
            kind,
            device: KeyDevice::Keyboard,
            physical,
            logical,
            character: None,
            synthesized: false,
        }
    }

    pub(in crate::views) fn keyboard_holding(keys: &[(u64, u64)]) -> HeldModifiers {
        keys.iter().copied().collect()
    }

    #[test]
    fn a_modifier_pressed_while_the_program_was_not_told_goes_down_first() {
        let mut told = ToldModifiers::default();
        let events = told.catch_up(&keyboard_holding(&[SHIFT]), 42);
        let [event] = &events[..] else {
            panic!("one key goes down: {events:?}");
        };
        assert_eq!((event.time, event.kind), (42, KeyKind::Down));
        assert_eq!(event.device, KeyDevice::Keyboard);
        assert_eq!((event.physical, event.logical), SHIFT);
        assert_eq!(event.character, None);
        assert!(event.synthesized);
    }

    #[test]
    fn a_modifier_released_while_the_program_was_not_told_comes_up_as_it_went_down() {
        let mut told = ToldModifiers::default();
        told.record(&key_event(KeyKind::Down, META));
        let events = told.catch_up(&keyboard_holding(&[]), 7);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, KeyKind::Up);
        assert_eq!((events[0].physical, events[0].logical), META);
        assert!(events[0].synthesized);
    }

    #[test]
    fn nothing_is_made_up_while_the_program_was_told_right() {
        let mut told = ToldModifiers::default();
        told.record(&key_event(KeyKind::Down, SHIFT));
        assert!(told.catch_up(&keyboard_holding(&[SHIFT]), 0).is_empty());
        told.record(&key_event(KeyKind::Up, SHIFT));
        assert!(told.catch_up(&keyboard_holding(&[]), 0).is_empty());
    }

    #[test]
    fn a_catch_up_counts_as_told() {
        let mut told = ToldModifiers::default();
        let held = keyboard_holding(&[SHIFT, META]);
        assert_eq!(told.catch_up(&held, 0).len(), 2);
        assert!(told.catch_up(&held, 1).is_empty());
    }

    #[test]
    fn keys_that_are_not_modifiers_are_left_alone() {
        let mut told = ToldModifiers::default();
        told.record(&key_event(KeyKind::Down, KEY_A));
        assert!(told.catch_up(&keyboard_holding(&[KEY_A]), 0).is_empty());
        assert!(told.catch_up(&keyboard_holding(&[]), 0).is_empty());
    }

    #[test]
    fn the_keyboard_held_keeps_only_its_modifier_keys() {
        let held = keyboard_holding(&[KEY_A, SHIFT]);
        assert_eq!(held.0, HashMap::from([SHIFT]));
    }
}
