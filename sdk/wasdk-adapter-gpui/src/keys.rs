//! A view's keys as GPUI's keystrokes.
//!
//! A wasdk key is Flutter's `KeyData`: its `logical` key, what it means under the user's layout (a
//! character key's lower-case code point, else Flutter's number for a named key), and the text it
//! typed. GPUI's keystroke is a key name (`"a"`, `"enter"`, `"left"`, `"f1"`), the modifiers held,
//! and the text typed (`key_char`). A key names no modifiers, so they are kept here from the
//! modifier keys themselves: WaOS sends a modifier that changed unseen as a synthesized key before
//! the event that showed it.

use gpui::{Capslock, Keystroke, Modifiers};
use wasdk::keys::logical;
use wasdk::views::{KeyEvent, KeyKind};

/// What one key means to GPUI.
pub(crate) enum KeyMeaning {
    /// A modifier went down or up; the modifiers held are now these.
    ModifiersChanged(Modifiers, Capslock),
    /// Any other key, pressed, held or released.
    Keystroke { keystroke: Keystroke, kind: KeyKind },
    /// A key GPUI has no name for.
    Unnamed,
}

/// The modifiers held, kept from the modifier keys.
#[derive(Default)]
pub(crate) struct HeldModifiers {
    modifiers: Modifiers,
    capslock: Capslock,
    /// Left and right of each, so releasing one while the other is held keeps it held.
    left_right: [bool; 8],
}

impl HeldModifiers {
    pub(crate) fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    pub(crate) fn capslock(&self) -> Capslock {
        self.capslock
    }

    /// What `event` means, keeping the modifiers it changes.
    pub(crate) fn meaning(&mut self, event: &KeyEvent) -> KeyMeaning {
        if let Some(slot) = modifier_slot(event.logical) {
            self.left_right[slot] = event.kind != KeyKind::Up;
            self.modifiers = self.held();
            return KeyMeaning::ModifiersChanged(self.modifiers, self.capslock);
        }
        if event.logical == logical::CAPS_LOCK {
            if event.kind == KeyKind::Down {
                self.capslock.on = !self.capslock.on;
            }
            return KeyMeaning::ModifiersChanged(self.modifiers, self.capslock);
        }
        let Some(key) = key_name(event.logical) else {
            return KeyMeaning::Unnamed;
        };
        let key_char = typed_text(event, &self.modifiers);
        KeyMeaning::Keystroke {
            keystroke: Keystroke {
                modifiers: self.modifiers,
                key,
                key_char,
            },
            kind: event.kind,
        }
    }

    fn held(&self) -> Modifiers {
        let [
            control_l,
            control_r,
            shift_l,
            shift_r,
            alt_l,
            alt_r,
            meta_l,
            meta_r,
        ] = self.left_right;
        Modifiers {
            control: control_l || control_r,
            alt: alt_l || alt_r,
            shift: shift_l || shift_r,
            platform: meta_l || meta_r,
            function: false,
        }
    }
}

fn modifier_slot(key: u64) -> Option<usize> {
    [
        logical::CONTROL_LEFT,
        logical::CONTROL_RIGHT,
        logical::SHIFT_LEFT,
        logical::SHIFT_RIGHT,
        logical::ALT_LEFT,
        logical::ALT_RIGHT,
        logical::META_LEFT,
        logical::META_RIGHT,
    ]
    .iter()
    .position(|modifier| *modifier == key)
}

/// The text a key typed, as GPUI's `key_char`: none for a chord with Command or Control, which
/// types nothing, unless it is AltGr's Control and Alt.
fn typed_text(event: &KeyEvent, modifiers: &Modifiers) -> Option<String> {
    let chord = (modifiers.platform || modifiers.control) && !(modifiers.control && modifiers.alt);
    if chord {
        return None;
    }
    event
        .character
        .clone()
        .filter(|text| !text.chars().any(char::is_control))
}

/// GPUI's name of a logical key: a character key by its character, a named key by GPUI's name.
fn key_name(key: u64) -> Option<String> {
    if key & logical::PLANE_MASK == logical::UNICODE_PLANE {
        let character = char::from_u32((key & logical::VALUE_MASK) as u32)?;
        return Some(match character {
            ' ' => "space".to_owned(),
            other => other.to_lowercase().collect(),
        });
    }
    let name = match key {
        logical::ENTER | logical::NUMPAD_ENTER => "enter",
        logical::BACKSPACE => "backspace",
        logical::TAB => "tab",
        logical::ESCAPE => "escape",
        logical::DELETE => "delete",
        logical::ARROW_LEFT => "left",
        logical::ARROW_RIGHT => "right",
        logical::ARROW_UP => "up",
        logical::ARROW_DOWN => "down",
        logical::HOME => "home",
        logical::END => "end",
        logical::PAGE_UP => "pageup",
        logical::PAGE_DOWN => "pagedown",
        logical::INSERT => "insert",
        logical::CONTEXT_MENU => "menu",
        _ => return function_key(key),
    };
    Some(name.to_owned())
}

fn function_key(key: u64) -> Option<String> {
    let number = key.checked_sub(logical::F1)? + 1;
    (number <= 24).then(|| format!("f{number}"))
}
