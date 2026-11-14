//! What a key types into a terminal: the bytes xterm sends for it, which every program run in a
//! terminal expects. Plain characters are not here: they are typed as text, through the view's
//! text input, so an input method's text and a key's go the same way.
//!
//! The invariant a later editor will break: a key that answers `None` must be left to the
//! platform, which types its character or hands it to a menu; answering bytes for a key takes it
//! from both.

use waft_services::LogicalKeyboardKey as Key;

/// The modifier keys held with a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Modifiers {
    pub(crate) shift: bool,
    pub(crate) control: bool,
    pub(crate) alt: bool,
    pub(crate) meta: bool,
}

impl Modifiers {
    /// xterm's number for the modifiers held, which follows a key's own in its sequence; none
    /// when no modifier is held.
    fn xterm_parameter(self) -> Option<u8> {
        let bits = u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control);
        (bits > 0).then_some(bits + 1)
    }
}

/// How the terminal's program asked for keys to be sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyModes {
    /// DECCKM: the arrows, Home and End are sent as `ESC O` sequences, as full-screen programs
    /// ask.
    pub(crate) application_cursor: bool,
    /// Alt with a character sends Escape before it, as Meta does: off where Alt is the
    /// keyboard's way to other characters, as Option is on a Mac.
    pub(crate) alt_sends_escape: bool,
}

/// The bytes `key` sends with `modifiers` held; none for a key that types its character, and for
/// a chord with Command, which is a menu's.
pub(crate) fn bytes_for_key(key: Key, modifiers: Modifiers, modes: KeyModes) -> Option<Vec<u8>> {
    if modifiers.meta {
        return None;
    }
    editing_key(key, modifiers)
        .or_else(|| cursor_key(key, modifiers, modes))
        .or_else(|| tilde_key(key, modifiers))
        .or_else(|| function_key(key, modifiers))
        .or_else(|| character_chord(key, modifiers, modes))
}

/// Enter, Tab, Backspace and Escape.
fn editing_key(key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let bytes: &[u8] = match key {
        Key::ENTER | Key::NUMPAD_ENTER => b"\r",
        Key::TAB if modifiers.shift => return Some(b"\x1b[Z".to_vec()),
        Key::TAB => b"\t",
        Key::BACKSPACE if modifiers.control => b"\x08",
        Key::BACKSPACE => b"\x7f",
        Key::ESCAPE => b"\x1b",
        _ => return None,
    };
    Some(with_escape_for_alt(bytes, modifiers))
}

/// `bytes` after an Escape where Alt is held: how a terminal sends Meta.
fn with_escape_for_alt(bytes: &[u8], modifiers: Modifiers) -> Vec<u8> {
    let mut sent = Vec::with_capacity(bytes.len() + 1);
    if modifiers.alt {
        sent.push(0x1b);
    }
    sent.extend_from_slice(bytes);
    sent
}

/// The arrows, Home and End: a letter after `ESC [`, or `ESC O` in application mode, with the
/// modifiers' number before it where one is held. Alt with Left or Right moves by a word, as
/// macOS's terminals send it, since shells bind `ESC b` and `ESC f` and not xterm's sequence.
fn cursor_key(key: Key, modifiers: Modifiers, modes: KeyModes) -> Option<Vec<u8>> {
    let letter = match key {
        Key::ARROW_UP => 'A',
        Key::ARROW_DOWN => 'B',
        Key::ARROW_RIGHT => 'C',
        Key::ARROW_LEFT => 'D',
        Key::HOME => 'H',
        Key::END => 'F',
        _ => return None,
    };
    let alt_alone = modifiers.alt && !modifiers.shift && !modifiers.control;
    let sequence = match (letter, alt_alone, modifiers.xterm_parameter()) {
        ('C', true, _) => "\x1bf".to_owned(),
        ('D', true, _) => "\x1bb".to_owned(),
        (_, _, Some(parameter)) => format!("\x1b[1;{parameter}{letter}"),
        (_, _, None) if modes.application_cursor => format!("\x1bO{letter}"),
        (_, _, None) => format!("\x1b[{letter}"),
    };
    Some(sequence.into_bytes())
}

/// Insert, Delete, Page Up and Page Down: a number and `~` after `ESC [`.
fn tilde_key(key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let number = match key {
        Key::INSERT => 2,
        Key::DELETE => 3,
        Key::PAGE_UP => 5,
        Key::PAGE_DOWN => 6,
        _ => return None,
    };
    Some(tilde_sequence(number, modifiers))
}

fn tilde_sequence(number: u8, modifiers: Modifiers) -> Vec<u8> {
    match modifiers.xterm_parameter() {
        Some(parameter) => format!("\x1b[{number};{parameter}~"),
        None => format!("\x1b[{number}~"),
    }
    .into_bytes()
}

/// F1 to F12: F1 to F4 a letter after `ESC O`, the rest a number and `~`.
fn function_key(key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    const LETTERED: [(Key, char); 4] = [
        (Key::F1, 'P'),
        (Key::F2, 'Q'),
        (Key::F3, 'R'),
        (Key::F4, 'S'),
    ];
    const NUMBERED: [(Key, u8); 8] = [
        (Key::F5, 15),
        (Key::F6, 17),
        (Key::F7, 18),
        (Key::F8, 19),
        (Key::F9, 20),
        (Key::F10, 21),
        (Key::F11, 23),
        (Key::F12, 24),
    ];
    if let Some((_, letter)) = LETTERED.iter().find(|(lettered, _)| *lettered == key) {
        let sequence = match modifiers.xterm_parameter() {
            Some(parameter) => format!("\x1b[1;{parameter}{letter}"),
            None => format!("\x1bO{letter}"),
        };
        return Some(sequence.into_bytes());
    }
    let (_, number) = NUMBERED.iter().find(|(numbered, _)| *numbered == key)?;
    Some(tilde_sequence(*number, modifiers))
}

/// A character's key with Control, which sends the character's control code, or with Alt where
/// Alt sends Escape; none for a character typed as it is.
fn character_chord(key: Key, modifiers: Modifiers, modes: KeyModes) -> Option<Vec<u8>> {
    let character = char::from_u32(u32::try_from(key.key_id).ok()?).filter(char::is_ascii)?;
    let escaped = modifiers.alt && modes.alt_sends_escape;
    let byte = if modifiers.control {
        control_code(character)?
    } else if escaped {
        typed_ascii(character, modifiers.shift)
    } else {
        return None;
    };
    let mut sent = Vec::with_capacity(2);
    if escaped {
        sent.push(0x1b);
    }
    sent.push(byte);
    Some(sent)
}

/// The control code Control makes of `character`, as a VT100's keyboard did: a letter's place in
/// the alphabet, and the punctuation and digits that stand for the codes with no letter.
fn control_code(character: char) -> Option<u8> {
    let code = match character {
        'a'..='z' => character as u8 - b'a' + 1,
        ' ' | '2' | '@' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '6' | '^' => 0x1e,
        '-' | '7' | '/' | '_' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    };
    Some(code)
}

/// The ASCII character a key types, a letter in upper case with Shift.
fn typed_ascii(character: char, shift: bool) -> u8 {
    let typed = if shift {
        character.to_ascii_uppercase()
    } else {
        character
    };
    typed as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        control: false,
        alt: false,
        meta: false,
    };
    const CONTROL: Modifiers = Modifiers {
        control: true,
        ..NONE
    };
    const ALT: Modifiers = Modifiers { alt: true, ..NONE };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..NONE
    };

    fn sent(key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
        bytes_for_key(key, modifiers, KeyModes::default())
    }

    #[test]
    fn a_character_alone_is_left_to_be_typed() {
        assert_eq!(sent(Key::KEY_A, NONE), None);
        assert_eq!(sent(Key::KEY_A, SHIFT), None);
        assert_eq!(sent(Key::SPACE, NONE), None);
    }

    #[test]
    fn control_with_a_letter_sends_its_control_code() {
        assert_eq!(sent(Key::KEY_C, CONTROL), Some(vec![0x03]));
        assert_eq!(sent(Key::KEY_D, CONTROL), Some(vec![0x04]));
        assert_eq!(sent(Key::KEY_Z, CONTROL), Some(vec![0x1a]));
        assert_eq!(sent(Key::SPACE, CONTROL), Some(vec![0x00]));
        assert_eq!(sent(Key::BRACKET_LEFT, CONTROL), Some(vec![0x1b]));
    }

    #[test]
    fn a_chord_with_command_is_left_to_the_menus() {
        let command = Modifiers { meta: true, ..NONE };
        assert_eq!(sent(Key::KEY_C, command), None);
        assert_eq!(sent(Key::ARROW_UP, command), None);
    }

    #[test]
    fn the_arrows_follow_the_cursor_mode() {
        assert_eq!(sent(Key::ARROW_UP, NONE), Some(b"\x1b[A".to_vec()));
        let application = KeyModes {
            application_cursor: true,
            ..KeyModes::default()
        };
        assert_eq!(
            bytes_for_key(Key::ARROW_UP, NONE, application),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            bytes_for_key(Key::ARROW_UP, CONTROL, application),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn alt_with_left_and_right_moves_by_a_word() {
        assert_eq!(sent(Key::ARROW_LEFT, ALT), Some(b"\x1bb".to_vec()));
        assert_eq!(sent(Key::ARROW_RIGHT, ALT), Some(b"\x1bf".to_vec()));
        assert_eq!(sent(Key::ARROW_UP, ALT), Some(b"\x1b[1;3A".to_vec()));
    }

    #[test]
    fn the_editing_keys_send_what_a_shell_expects() {
        assert_eq!(sent(Key::ENTER, NONE), Some(b"\r".to_vec()));
        assert_eq!(sent(Key::BACKSPACE, NONE), Some(vec![0x7f]));
        assert_eq!(sent(Key::BACKSPACE, ALT), Some(vec![0x1b, 0x7f]));
        assert_eq!(sent(Key::TAB, SHIFT), Some(b"\x1b[Z".to_vec()));
        assert_eq!(sent(Key::DELETE, NONE), Some(b"\x1b[3~".to_vec()));
        assert_eq!(sent(Key::PAGE_UP, NONE), Some(b"\x1b[5~".to_vec()));
    }

    #[test]
    fn the_function_keys_send_xterms_sequences() {
        assert_eq!(sent(Key::F1, NONE), Some(b"\x1bOP".to_vec()));
        assert_eq!(sent(Key::F5, NONE), Some(b"\x1b[15~".to_vec()));
        assert_eq!(sent(Key::F12, SHIFT), Some(b"\x1b[24;2~".to_vec()));
    }

    #[test]
    fn alt_sends_escape_only_where_it_is_meta() {
        assert_eq!(sent(Key::KEY_B, ALT), None);
        let meta = KeyModes {
            alt_sends_escape: true,
            ..KeyModes::default()
        };
        assert_eq!(
            bytes_for_key(Key::KEY_B, ALT, meta),
            Some(b"\x1bb".to_vec())
        );
    }
}
