//! A view's keys as egui's: the key each logical id names, and the modifiers held.
//!
//! A view's key carries no modifiers, as Flutter's `KeyData` carries none, while every key and
//! pointer event of egui's names them. So the modifiers are kept here, from the modifier keys as
//! they go down and come up; the platform sends the ones it saw change out of the program's sight
//! as synthesized keys, which keeps this right across a click in another window.

use wasdk::keys::logical;
use wasdk::views::{KeyEvent, KeyKind};

/// The modifier keys held now.
pub(super) struct Keyboard {
    held: Vec<ModifierKey>,
    macos: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Modifier {
    Control,
    Shift,
    Alt,
    Meta,
}

/// One modifier key, the left one or the right one.
#[derive(Clone, Copy, PartialEq)]
struct ModifierKey {
    logical: u64,
    modifier: Modifier,
}

impl Keyboard {
    /// A keyboard with nothing held, on macOS or not: where ⌘ is egui's command key.
    pub(super) fn new(macos: bool) -> Keyboard {
        Keyboard {
            held: Vec::new(),
            macos,
        }
    }

    /// Notes `event` when it is a modifier key; whether it was one.
    pub(super) fn track(&mut self, event: &KeyEvent) -> bool {
        let Some(key) = modifier_key(event.logical) else {
            return false;
        };
        self.held.retain(|held| held.logical != key.logical);
        if event.kind != KeyKind::Up {
            self.held.push(key);
        }
        true
    }

    pub(super) fn modifiers(&self) -> egui::Modifiers {
        let holds = |modifier| self.held.iter().any(|key| key.modifier == modifier);
        let (control, meta) = (holds(Modifier::Control), holds(Modifier::Meta));
        egui::Modifiers {
            alt: holds(Modifier::Alt),
            ctrl: control,
            shift: holds(Modifier::Shift),
            mac_cmd: self.macos && meta,
            command: if self.macos { meta } else { control },
        }
    }
}

fn modifier_key(key: u64) -> Option<ModifierKey> {
    let modifier = match key {
        logical::CONTROL_LEFT | logical::CONTROL_RIGHT => Modifier::Control,
        logical::SHIFT_LEFT | logical::SHIFT_RIGHT => Modifier::Shift,
        logical::ALT_LEFT | logical::ALT_RIGHT => Modifier::Alt,
        logical::META_LEFT | logical::META_RIGHT => Modifier::Meta,
        _ => return None,
    };
    Some(ModifierKey {
        logical: key,
        modifier,
    })
}

/// The egui key a logical id names; none for a key egui has no name for.
pub(super) fn key_of(key: u64) -> Option<egui::Key> {
    if key & logical::PLANE_MASK == logical::UNICODE_PLANE {
        return character_key(key);
    }
    named_key(key)
}

/// A character key, which Flutter names by its lower-case code point and egui by the character.
fn character_key(code_point: u64) -> Option<egui::Key> {
    let character = char::from_u32(u32::try_from(code_point).ok()?)?;
    egui::Key::from_name(character.encode_utf8(&mut [0; 4]))
}

/// A key without a character, the number pad's among them, which egui names as their characters.
fn named_key(key: u64) -> Option<egui::Key> {
    use egui::Key;
    Some(match key {
        logical::BACKSPACE => Key::Backspace,
        logical::TAB => Key::Tab,
        logical::ENTER | logical::NUMPAD_ENTER => Key::Enter,
        logical::ESCAPE => Key::Escape,
        logical::DELETE => Key::Delete,
        logical::ARROW_DOWN => Key::ArrowDown,
        logical::ARROW_LEFT => Key::ArrowLeft,
        logical::ARROW_RIGHT => Key::ArrowRight,
        logical::ARROW_UP => Key::ArrowUp,
        logical::END => Key::End,
        logical::HOME => Key::Home,
        logical::PAGE_DOWN => Key::PageDown,
        logical::PAGE_UP => Key::PageUp,
        logical::COPY => Key::Copy,
        logical::CUT => Key::Cut,
        logical::INSERT => Key::Insert,
        logical::PASTE => Key::Paste,
        logical::F1 => Key::F1,
        logical::F2 => Key::F2,
        logical::F3 => Key::F3,
        logical::F4 => Key::F4,
        logical::F5 => Key::F5,
        logical::F6 => Key::F6,
        logical::F7 => Key::F7,
        logical::F8 => Key::F8,
        logical::F9 => Key::F9,
        logical::F10 => Key::F10,
        logical::F11 => Key::F11,
        logical::F12 => Key::F12,
        logical::NUMPAD_ADD => Key::Plus,
        logical::NUMPAD_SUBTRACT => Key::Minus,
        logical::NUMPAD_DECIMAL => Key::Period,
        logical::NUMPAD_DIVIDE => Key::Slash,
        logical::NUMPAD_EQUAL => Key::Equals,
        logical::NUMPAD0 => Key::Num0,
        logical::NUMPAD1 => Key::Num1,
        logical::NUMPAD2 => Key::Num2,
        logical::NUMPAD3 => Key::Num3,
        logical::NUMPAD4 => Key::Num4,
        logical::NUMPAD5 => Key::Num5,
        logical::NUMPAD6 => Key::Num6,
        logical::NUMPAD7 => Key::Num7,
        logical::NUMPAD8 => Key::Num8,
        logical::NUMPAD9 => Key::Num9,
        _ => return None,
    })
}
