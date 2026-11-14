//! Where the keys of the keyboard drawn are: as many white keys as its width takes from its
//! lowest note, a C, with the black keys over the gaps between them, and which key a point is on.

use crate::pitch::{Note, OCTAVE};

/// How wide a white key aims to be, in logical pixels: a finger's width.
const WHITE_KEY_WIDTH: f64 = 44.0;

/// The fewest and the most white keys drawn: an octave, and four.
const FEWEST_WHITE_KEYS: usize = 7;
const MOST_WHITE_KEYS: usize = 29;

/// A black key's width and height, as parts of a white key's.
const BLACK_KEY_WIDTH: f64 = 0.6;
const BLACK_KEY_HEIGHT: f64 = 0.62;

/// The semitones above a C of an octave's white keys.
const WHITE_STEPS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KeyboardLayout {
    /// The lowest key, a C.
    lowest: Note,
    white_keys: usize,
    width: f64,
    height: f64,
}

/// A key's place, from the keyboard's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KeyPlace {
    pub(crate) left: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl KeyboardLayout {
    /// The keyboard from `lowest`, a C, in a room `width` by `height`.
    pub(crate) fn fitting(lowest: Note, width: f64, height: f64) -> KeyboardLayout {
        let fitting = (width / WHITE_KEY_WIDTH).round() as usize;
        let up_to_the_highest = WHITE_STEPS.len() * usize::from((127 - lowest.0) / OCTAVE);
        KeyboardLayout {
            lowest,
            white_keys: fitting
                .clamp(FEWEST_WHITE_KEYS, MOST_WHITE_KEYS)
                .min(up_to_the_highest),
            width,
            height,
        }
    }

    /// The white keys, from the lowest, each with its place.
    pub(crate) fn white_keys(&self) -> impl Iterator<Item = (Note, KeyPlace)> + '_ {
        (0..self.white_keys).map(|index| (self.white_note(index), self.white_place(index)))
    }

    /// The black keys, from the lowest, each with its place: one after each white key whose
    /// next semitone is black, the last white key aside.
    pub(crate) fn black_keys(&self) -> impl Iterator<Item = (Note, KeyPlace)> + '_ {
        (0..self.white_keys.saturating_sub(1)).filter_map(|index| {
            let note = Note(self.white_note(index).0 + 1);
            note.is_black().then(|| (note, self.black_place(index)))
        })
    }

    /// The key at a point, the black keys lying over the white.
    pub(crate) fn note_at(&self, x: f64, y: f64) -> Option<Note> {
        if !(0.0..self.width).contains(&x) || !(0.0..self.height).contains(&y) {
            return None;
        }
        let on_black = self.black_keys().find(|(_, place)| {
            y < place.height && (place.left..place.left + place.width).contains(&x)
        });
        let white = (x / self.white_width()) as usize;
        Some(on_black.map_or(
            self.white_note(white.min(self.white_keys - 1)),
            |(note, _)| note,
        ))
    }

    fn white_width(&self) -> f64 {
        self.width / self.white_keys as f64
    }

    /// The note of the white key `index` keys up from the lowest.
    fn white_note(&self, index: usize) -> Note {
        let octaves = (index / WHITE_STEPS.len()) as u8;
        Note(self.lowest.0 + octaves * OCTAVE + WHITE_STEPS[index % WHITE_STEPS.len()])
    }

    fn white_place(&self, index: usize) -> KeyPlace {
        KeyPlace {
            left: index as f64 * self.white_width(),
            width: self.white_width(),
            height: self.height,
        }
    }

    /// The place of the black key over the gap after the white key `index`.
    fn black_place(&self, index: usize) -> KeyPlace {
        let width = self.white_width() * BLACK_KEY_WIDTH;
        KeyPlace {
            left: (index + 1) as f64 * self.white_width() - width / 2.0,
            width,
            height: self.height * BLACK_KEY_HEIGHT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_octaves() -> KeyboardLayout {
        KeyboardLayout::fitting(Note::c(3), 14.0 * WHITE_KEY_WIDTH, 200.0)
    }

    #[test]
    fn a_width_takes_as_many_white_keys_as_fit_a_finger_each() {
        let layout = two_octaves();
        let whites: Vec<Note> = layout.white_keys().map(|(note, _)| note).collect();
        assert_eq!(whites.len(), 14);
        assert_eq!(whites[0], Note(48));
        assert_eq!(whites[7], Note(60));
        assert_eq!(whites[13], Note(71));
        assert!(whites.iter().all(|note| !note.is_black()));
    }

    #[test]
    fn a_narrow_room_still_has_an_octave_and_a_wide_one_no_more_than_four() {
        let narrow = KeyboardLayout::fitting(Note::c(3), 120.0, 200.0);
        assert_eq!(narrow.white_keys().count(), FEWEST_WHITE_KEYS);
        let wide = KeyboardLayout::fitting(Note::c(3), 5_000.0, 200.0);
        assert_eq!(wide.white_keys().count(), MOST_WHITE_KEYS);
    }

    #[test]
    fn the_keyboard_ends_where_the_notes_do() {
        let high = KeyboardLayout::fitting(Note::c(8), 5_000.0, 200.0);
        let highest = high.white_keys().last().unwrap().0;
        assert!(highest.0 <= 127, "ends at {highest:?}");
    }

    #[test]
    fn black_keys_lie_over_the_gaps_but_not_between_e_and_f_or_b_and_c() {
        let layout = two_octaves();
        let blacks: Vec<Note> = layout.black_keys().map(|(note, _)| note).collect();
        assert_eq!(blacks, [49, 51, 54, 56, 58, 61, 63, 66, 68, 70].map(Note));
        let (_, c_sharp) = layout.black_keys().next().unwrap();
        assert_eq!(c_sharp.left + c_sharp.width / 2.0, WHITE_KEY_WIDTH);
    }

    #[test]
    fn a_point_is_on_the_black_key_over_it_or_the_white_key_under() {
        let layout = two_octaves();
        assert_eq!(layout.note_at(5.0, 10.0), Some(Note(48)));
        assert_eq!(
            layout.note_at(WHITE_KEY_WIDTH, 10.0),
            Some(Note(49)),
            "C sharp, up high"
        );
        assert_eq!(
            layout.note_at(WHITE_KEY_WIDTH + 1.0, 190.0),
            Some(Note(50)),
            "D, down low"
        );
        assert_eq!(
            layout.note_at(14.0 * WHITE_KEY_WIDTH - 1.0, 100.0),
            Some(Note(71))
        );
    }

    #[test]
    fn a_point_off_the_keyboard_is_on_no_key() {
        let layout = two_octaves();
        assert_eq!(layout.note_at(-1.0, 10.0), None);
        assert_eq!(layout.note_at(10.0, 200.0), None);
        assert_eq!(layout.note_at(14.0 * WHITE_KEY_WIDTH, 10.0), None);
    }
}
