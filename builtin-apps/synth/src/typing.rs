//! The computer's keys as a piano's: a row of its keyboard as the white keys, the row above as
//! the black ones between them, and two keys that move the keyboard an octave. By where a key
//! is, not what it types, so the row lies the same under the hand on any layout.

use waft_services::PhysicalKeyboardKey;

/// What a key of the computer's does to the instrument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypedKey {
    /// Plays the note this many semitones above the keyboard's lowest.
    Semitone(u8),
    /// Moves the keyboard by this many octaves.
    Octaves(i8),
}

/// The keys that play, each with the semitones it plays above the keyboard's lowest note and
/// what a US keyboard prints on it: from A, a C, to the apostrophe, the F an octave above.
const PLAYING: [(PhysicalKeyboardKey, u8, &str); 18] = [
    (PhysicalKeyboardKey::KEY_A, 0, "A"),
    (PhysicalKeyboardKey::KEY_W, 1, "W"),
    (PhysicalKeyboardKey::KEY_S, 2, "S"),
    (PhysicalKeyboardKey::KEY_E, 3, "E"),
    (PhysicalKeyboardKey::KEY_D, 4, "D"),
    (PhysicalKeyboardKey::KEY_F, 5, "F"),
    (PhysicalKeyboardKey::KEY_T, 6, "T"),
    (PhysicalKeyboardKey::KEY_G, 7, "G"),
    (PhysicalKeyboardKey::KEY_Y, 8, "Y"),
    (PhysicalKeyboardKey::KEY_H, 9, "H"),
    (PhysicalKeyboardKey::KEY_U, 10, "U"),
    (PhysicalKeyboardKey::KEY_J, 11, "J"),
    (PhysicalKeyboardKey::KEY_K, 12, "K"),
    (PhysicalKeyboardKey::KEY_O, 13, "O"),
    (PhysicalKeyboardKey::KEY_L, 14, "L"),
    (PhysicalKeyboardKey::KEY_P, 15, "P"),
    (PhysicalKeyboardKey::SEMICOLON, 16, ";"),
    (PhysicalKeyboardKey::QUOTE, 17, "'"),
];

/// What `key` does, if anything.
pub(crate) fn typed(key: PhysicalKeyboardKey) -> Option<TypedKey> {
    if key == PhysicalKeyboardKey::KEY_Z {
        return Some(TypedKey::Octaves(-1));
    }
    if key == PhysicalKeyboardKey::KEY_X {
        return Some(TypedKey::Octaves(1));
    }
    PLAYING
        .iter()
        .find(|(playing, _, _)| *playing == key)
        .map(|(_, semitone, _)| TypedKey::Semitone(*semitone))
}

/// What a US keyboard prints on the key that plays `semitone` above the lowest note.
pub(crate) fn printed_on_key_for(semitone: u8) -> Option<&'static str> {
    PLAYING
        .iter()
        .find(|(_, played, _)| *played == semitone)
        .map(|(_, _, printed)| *printed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_row_is_the_white_keys_and_the_row_above_the_black() {
        assert_eq!(
            typed(PhysicalKeyboardKey::KEY_A),
            Some(TypedKey::Semitone(0))
        );
        assert_eq!(
            typed(PhysicalKeyboardKey::KEY_W),
            Some(TypedKey::Semitone(1))
        );
        assert_eq!(
            typed(PhysicalKeyboardKey::KEY_K),
            Some(TypedKey::Semitone(12))
        );
        assert_eq!(typed(PhysicalKeyboardKey::KEY_Q), None);
    }

    #[test]
    fn z_and_x_move_the_keyboard_an_octave() {
        assert_eq!(
            typed(PhysicalKeyboardKey::KEY_Z),
            Some(TypedKey::Octaves(-1))
        );
        assert_eq!(
            typed(PhysicalKeyboardKey::KEY_X),
            Some(TypedKey::Octaves(1))
        );
    }

    #[test]
    fn every_semitone_of_the_row_has_a_key_and_no_two_share_one() {
        for semitone in 0..18 {
            assert!(
                printed_on_key_for(semitone).is_some(),
                "semitone {semitone}"
            );
        }
        assert_eq!(printed_on_key_for(18), None);
    }
}
