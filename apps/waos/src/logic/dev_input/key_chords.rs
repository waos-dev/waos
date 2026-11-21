//! A key, or a chord of keys pressed together, as `waos key` names it, and the key events a
//! keyboard sends for it, as the shell reports a user's keys, Waft's key events field for field:
//! each modifier going down in the order named, the key going down and coming up, and the
//! modifiers coming up in the opposite order. The keyboard is a US one, its keys the physical and
//! logical keys Waft names (`PhysicalKeyboardKey`, `LogicalKeyboardKey`), the modifiers its left
//! ones.
//!
//! A chord is names joined by `+`, the modifiers first, each in any case: `cmd` (or `command`,
//! `meta`), `ctrl` (`control`), `alt` (`option`) and `shift`; then the key, a modifier itself, a
//! letter, a digit, one of ``- = [ ] \ ; ' ` , . /``, or `enter` (`return`), `escape` (`esc`),
//! `tab`, `space`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`,
//! `pagedown` or `f1` to `f12`. A key going down carries the character it types as macOS gives
//! it: the one Shift picks, the character a letter or a symbol types under Command as without it,
//! and none under Control or Option, whose characters are the layout's and are not made up here;
//! a named key types none but `space`. A character key's logical key is its lower-case letter, or
//! the symbol Shift picks, as Waft's host takes it from the system.

use waft_services::{LogicalKeyboardKey, PhysicalKeyboardKey};
use waos::bindings::wasdk::platform::views::{KeyDevice, KeyEvent, KeyKind};

/// A modifier's key, and whether it changes the character a key types.
#[derive(Clone, Copy, PartialEq)]
enum Modifier {
    Meta,
    Control,
    Alt,
    Shift,
}

/// One key, as a keyboard reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    physical: u64,
    logical: u64,
    /// What it types, unshifted and shifted; none for a key that types nothing.
    characters: Option<(char, char)>,
}

/// The events of `chord`, stamped `now` on the program's clock; none for a chord whose names name
/// no key, or whose keys before the last are not modifiers.
pub(crate) fn key_events(chord: &str, now: u64) -> Option<Vec<KeyEvent>> {
    let names: Vec<String> = chord.split('+').map(str::to_lowercase).collect();
    let (key_name, modifier_names) = names.split_last()?;
    let modifiers = modifier_names
        .iter()
        .map(|name| modifier_named(name))
        .collect::<Option<Vec<Modifier>>>()?;
    let key = modifier_named(key_name)
        .map(modifier_key)
        .or_else(|| key_named(key_name))?;
    let mut events = Vec::new();
    for &modifier in &modifiers {
        events.push(event(KeyKind::Down, modifier_key(modifier), None, now));
    }
    let typed = typed(&key, &modifiers);
    let logical = logical_under(&key, &modifiers);
    events.push(event(KeyKind::Down, Key { logical, ..key }, typed, now));
    events.push(event(KeyKind::Up, Key { logical, ..key }, None, now));
    for &modifier in modifiers.iter().rev() {
        events.push(event(KeyKind::Up, modifier_key(modifier), None, now));
    }
    Some(events)
}

fn event(kind: KeyKind, key: Key, character: Option<String>, now: u64) -> KeyEvent {
    KeyEvent {
        time: now,
        kind,
        device: KeyDevice::Keyboard,
        physical: key.physical,
        logical: key.logical,
        character,
        synthesized: false,
    }
}

/// What `key` types going down while `modifiers` are held.
fn typed(key: &Key, modifiers: &[Modifier]) -> Option<String> {
    let (plain, shifted) = key.characters?;
    if modifiers.contains(&Modifier::Control) || modifiers.contains(&Modifier::Alt) {
        return None;
    }
    let character = if modifiers.contains(&Modifier::Shift) {
        shifted
    } else {
        plain
    };
    Some(character.to_string())
}

/// `key`'s logical key while `modifiers` are held: a symbol's is the one Shift picks, a letter's
/// stays its lower case.
fn logical_under(key: &Key, modifiers: &[Modifier]) -> u64 {
    match key.characters {
        Some((plain, shifted))
            if modifiers.contains(&Modifier::Shift) && !plain.is_ascii_alphabetic() =>
        {
            u64::from(shifted)
        }
        _ => key.logical,
    }
}

fn modifier_named(name: &str) -> Option<Modifier> {
    Some(match name {
        "cmd" | "command" | "meta" => Modifier::Meta,
        "ctrl" | "control" => Modifier::Control,
        "alt" | "option" => Modifier::Alt,
        "shift" => Modifier::Shift,
        _ => return None,
    })
}

fn modifier_key(modifier: Modifier) -> Key {
    let (physical, logical) = match modifier {
        Modifier::Meta => (
            PhysicalKeyboardKey::META_LEFT,
            LogicalKeyboardKey::META_LEFT,
        ),
        Modifier::Control => (
            PhysicalKeyboardKey::CONTROL_LEFT,
            LogicalKeyboardKey::CONTROL_LEFT,
        ),
        Modifier::Alt => (PhysicalKeyboardKey::ALT_LEFT, LogicalKeyboardKey::ALT_LEFT),
        Modifier::Shift => (
            PhysicalKeyboardKey::SHIFT_LEFT,
            LogicalKeyboardKey::SHIFT_LEFT,
        ),
    };
    named(physical, logical)
}

/// The key `name` names, but a modifier.
fn key_named(name: &str) -> Option<Key> {
    let mut characters = name.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        return character_key(character);
    }
    named_key(name)
}

/// The key that types `character` on a US keyboard, unshifted.
fn character_key(character: char) -> Option<Key> {
    if character.is_ascii_lowercase() {
        let offset = u64::from(character) - u64::from('a');
        return Some(Key {
            physical: PhysicalKeyboardKey::KEY_A.usb_hid_usage + offset,
            logical: LogicalKeyboardKey::KEY_A.key_id + offset,
            characters: Some((character, character.to_ascii_uppercase())),
        });
    }
    let (physical, shifted) = symbol_key(character)?;
    Some(Key {
        physical: physical.usb_hid_usage,
        logical: u64::from(character),
        characters: Some((character, shifted)),
    })
}

/// The physical key of a digit or a symbol on a US keyboard, and what it types shifted.
fn symbol_key(character: char) -> Option<(PhysicalKeyboardKey, char)> {
    Some(match character {
        '1' => (PhysicalKeyboardKey::DIGIT1, '!'),
        '2' => (PhysicalKeyboardKey::DIGIT2, '@'),
        '3' => (PhysicalKeyboardKey::DIGIT3, '#'),
        '4' => (PhysicalKeyboardKey::DIGIT4, '$'),
        '5' => (PhysicalKeyboardKey::DIGIT5, '%'),
        '6' => (PhysicalKeyboardKey::DIGIT6, '^'),
        '7' => (PhysicalKeyboardKey::DIGIT7, '&'),
        '8' => (PhysicalKeyboardKey::DIGIT8, '*'),
        '9' => (PhysicalKeyboardKey::DIGIT9, '('),
        '0' => (PhysicalKeyboardKey::DIGIT0, ')'),
        '-' => (PhysicalKeyboardKey::MINUS, '_'),
        '=' => (PhysicalKeyboardKey::EQUAL, '+'),
        '[' => (PhysicalKeyboardKey::BRACKET_LEFT, '{'),
        ']' => (PhysicalKeyboardKey::BRACKET_RIGHT, '}'),
        '\\' => (PhysicalKeyboardKey::BACKSLASH, '|'),
        ';' => (PhysicalKeyboardKey::SEMICOLON, ':'),
        '\'' => (PhysicalKeyboardKey::QUOTE, '"'),
        '`' => (PhysicalKeyboardKey::BACKQUOTE, '~'),
        ',' => (PhysicalKeyboardKey::COMMA, '<'),
        '.' => (PhysicalKeyboardKey::PERIOD, '>'),
        '/' => (PhysicalKeyboardKey::SLASH, '?'),
        _ => return None,
    })
}

/// A key that types nothing but `space`, by its name.
fn named_key(name: &str) -> Option<Key> {
    if let Some(number) = name.strip_prefix('f') {
        return function_key(number.parse().ok()?);
    }
    let (physical, logical) = match name {
        "enter" | "return" => (PhysicalKeyboardKey::ENTER, LogicalKeyboardKey::ENTER),
        "escape" | "esc" => (PhysicalKeyboardKey::ESCAPE, LogicalKeyboardKey::ESCAPE),
        "tab" => (PhysicalKeyboardKey::TAB, LogicalKeyboardKey::TAB),
        "space" => {
            return Some(Key {
                characters: Some((' ', ' ')),
                ..named(PhysicalKeyboardKey::SPACE, LogicalKeyboardKey::SPACE)
            });
        }
        "backspace" => (
            PhysicalKeyboardKey::BACKSPACE,
            LogicalKeyboardKey::BACKSPACE,
        ),
        "delete" => (PhysicalKeyboardKey::DELETE, LogicalKeyboardKey::DELETE),
        "up" => (PhysicalKeyboardKey::ARROW_UP, LogicalKeyboardKey::ARROW_UP),
        "down" => (
            PhysicalKeyboardKey::ARROW_DOWN,
            LogicalKeyboardKey::ARROW_DOWN,
        ),
        "left" => (
            PhysicalKeyboardKey::ARROW_LEFT,
            LogicalKeyboardKey::ARROW_LEFT,
        ),
        "right" => (
            PhysicalKeyboardKey::ARROW_RIGHT,
            LogicalKeyboardKey::ARROW_RIGHT,
        ),
        "home" => (PhysicalKeyboardKey::HOME, LogicalKeyboardKey::HOME),
        "end" => (PhysicalKeyboardKey::END, LogicalKeyboardKey::END),
        "pageup" => (PhysicalKeyboardKey::PAGE_UP, LogicalKeyboardKey::PAGE_UP),
        "pagedown" => (
            PhysicalKeyboardKey::PAGE_DOWN,
            LogicalKeyboardKey::PAGE_DOWN,
        ),
        _ => return None,
    };
    Some(named(physical, logical))
}

/// `F1` to `F12`.
fn function_key(number: u64) -> Option<Key> {
    if !(1..=12).contains(&number) {
        return None;
    }
    let offset = number - 1;
    Some(Key {
        physical: PhysicalKeyboardKey::F1.usb_hid_usage + offset,
        logical: LogicalKeyboardKey::F1.key_id + offset,
        characters: None,
    })
}

fn named(physical: PhysicalKeyboardKey, logical: LogicalKeyboardKey) -> Key {
    Key {
        physical: physical.usb_hid_usage,
        logical: logical.key_id,
        characters: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `chord` sends, each event as its kind, physical key, logical key and character.
    fn sent(chord: &str) -> Vec<(KeyKind, u64, u64, Option<String>)> {
        key_events(chord, 5)
            .unwrap_or_else(|| panic!("{chord} names keys"))
            .into_iter()
            .map(|event| {
                assert_eq!(event.time, 5);
                assert!(!event.synthesized);
                (event.kind, event.physical, event.logical, event.character)
            })
            .collect()
    }

    const META: (u64, u64) = (0x0007_00e3, 0x0002_0000_0106);
    const SHIFT: (u64, u64) = (0x0007_00e1, 0x0002_0000_0102);

    #[test]
    fn a_letter_goes_down_typing_itself_and_comes_up_typing_nothing() {
        assert_eq!(
            sent("a"),
            [
                (KeyKind::Down, 0x0007_0004, 0x61, Some("a".into())),
                (KeyKind::Up, 0x0007_0004, 0x61, None),
            ]
        );
    }

    #[test]
    fn a_chord_holds_its_modifiers_around_its_key_in_the_order_named() {
        assert_eq!(
            sent("cmd+shift+z"),
            [
                (KeyKind::Down, META.0, META.1, None),
                (KeyKind::Down, SHIFT.0, SHIFT.1, None),
                (KeyKind::Down, 0x0007_001d, 0x7a, Some("Z".into())),
                (KeyKind::Up, 0x0007_001d, 0x7a, None),
                (KeyKind::Up, SHIFT.0, SHIFT.1, None),
                (KeyKind::Up, META.0, META.1, None),
            ]
        );
    }

    #[test]
    fn command_types_the_plain_character_and_control_and_option_type_none() {
        assert_eq!(sent("cmd+s")[1].3.as_deref(), Some("s"));
        assert_eq!(sent("ctrl+s")[1].3, None);
        assert_eq!(sent("alt+s")[1].3, None);
    }

    #[test]
    fn shift_picks_a_symbols_character_and_its_logical_key() {
        let shifted = sent("shift+1");
        assert_eq!(
            shifted[1],
            (KeyKind::Down, 0x0007_001e, 0x21, Some("!".into()))
        );
        assert_eq!(
            shifted[2].2, 0x21,
            "its release reports the key it went down as"
        );
        assert_eq!(
            sent("1")[0],
            (KeyKind::Down, 0x0007_001e, 0x31, Some("1".into()))
        );
    }

    #[test]
    fn named_keys_type_nothing_but_space() {
        assert_eq!(
            sent("enter")[0],
            (KeyKind::Down, 0x0007_0028, 0x0001_0000_000d, None)
        );
        assert_eq!(sent("Escape")[0].1, 0x0007_0029, "names in any case");
        assert_eq!(sent("space")[0].3.as_deref(), Some(" "));
        assert_eq!(sent("left")[0].2, 0x0001_0000_0302);
        assert_eq!(
            sent("f5")[0],
            (KeyKind::Down, 0x0007_003e, 0x0001_0000_0805, None)
        );
    }

    #[test]
    fn a_modifier_alone_is_pressed_and_released() {
        assert_eq!(
            sent("shift"),
            [
                (KeyKind::Down, SHIFT.0, SHIFT.1, None),
                (KeyKind::Up, SHIFT.0, SHIFT.1, None),
            ]
        );
    }

    #[test]
    fn a_name_no_key_has_or_a_key_before_the_last_names_no_chord() {
        for chord in ["hyper+q", "a+b", "f13", "", "cmd+", "é"] {
            assert!(key_events(chord, 0).is_none(), "{chord:?}");
        }
    }
}
