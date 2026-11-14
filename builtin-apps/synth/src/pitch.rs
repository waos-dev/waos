//! A note of the keyboard: which key it is, how it sounds and what it is called.

/// A key of a piano by its MIDI number: 60 is middle C, and each step a semitone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Note(pub(crate) u8);

/// The A above middle C, which the others are tuned from, and its pitch in hertz.
const CONCERT_A: Note = Note(69);
const CONCERT_A_HERTZ: f32 = 440.0;

/// The semitones of an octave.
pub(crate) const OCTAVE: u8 = 12;

/// The semitones above a C that are black keys.
const BLACK: [u8; 5] = [1, 3, 6, 8, 10];

impl Note {
    /// The C that starts `octave`, as pianos number them: octave 4 starts at middle C.
    pub(crate) fn c(octave: u8) -> Note {
        Note((octave + 1) * OCTAVE)
    }

    /// Its pitch in hertz, in equal temperament.
    pub(crate) fn hertz(self) -> f32 {
        let semitones = f32::from(self.0) - f32::from(CONCERT_A.0);
        CONCERT_A_HERTZ * (semitones / f32::from(OCTAVE)).exp2()
    }

    pub(crate) fn is_black(self) -> bool {
        BLACK.contains(&(self.0 % OCTAVE))
    }

    /// The octave it is in, as pianos number them.
    pub(crate) fn octave(self) -> u8 {
        (self.0 / OCTAVE).saturating_sub(1)
    }

    /// Whether it is a C, the key an octave starts at.
    pub(crate) fn is_c(self) -> bool {
        self.0.is_multiple_of(OCTAVE)
    }

    /// The note `semitones` above it, where a keyboard has one.
    pub(crate) fn up(self, semitones: u8) -> Option<Note> {
        self.0
            .checked_add(semitones)
            .filter(|number| *number <= HIGHEST.0)
            .map(Note)
    }
}

/// The highest note there is a number for.
pub(crate) const HIGHEST: Note = Note(127);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concert_a_is_440_hertz_and_an_octave_doubles_it() {
        assert_eq!(Note(69).hertz(), 440.0);
        assert!((Note(81).hertz() - 880.0).abs() < 1e-3);
        assert!((Note(60).hertz() - 261.626).abs() < 1e-2);
    }

    #[test]
    fn middle_c_starts_octave_4_and_is_white() {
        assert_eq!(Note::c(4), Note(60));
        assert_eq!(Note(60).octave(), 4);
        assert!(Note(60).is_c());
        assert!(!Note(60).is_black());
        assert!(Note(61).is_black());
    }

    #[test]
    fn there_is_no_note_above_the_highest() {
        assert_eq!(Note(120).up(7), Some(Note(127)));
        assert_eq!(Note(120).up(8), None);
    }
}
