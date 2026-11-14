//! The notes of one preset sounding together: its voices, mixed, through its effects.
//!
//! The invariant: no more than [`MOST_VOICES`] voices play in full at once. A key pressed
//! beyond that cuts the voice that has been going longest, a let-go one first.

use fundsp::prelude32::{AudioUnit, BufferArray, Shared, U1, U2, shared};

use crate::pitch::Note;
use crate::presets::{Preset, VoiceControls};
use crate::tone::Tone;
use crate::voice::{CHUNK, Voice};

/// The most voices that play in full at once.
const MOST_VOICES: usize = 12;

pub(crate) struct Layer {
    preset: Preset,
    sample_rate: f64,
    tone: Tone,
    /// The filter's cutoff and Q, which every voice reads as it plays.
    cutoff: Shared,
    q: Shared,
    /// The voices sounding, the longest-going first.
    voices: Vec<Voice>,
    effects: Box<dyn AudioUnit>,
    /// The voices mixed, and what the effects make of them, the left side and the right.
    mixed: BufferArray<U1>,
    played: BufferArray<U2>,
    one_voice: BufferArray<U1>,
}

impl Layer {
    /// A layer of `preset` with no note sounding, played with `tone` at `sample_rate` frames a
    /// second.
    pub(crate) fn new(preset: Preset, tone: Tone, sample_rate: f64) -> Layer {
        let mut effects = preset.effects();
        effects.set_sample_rate(sample_rate);
        effects.allocate();
        Layer {
            preset,
            sample_rate,
            tone,
            cutoff: shared(tone.cutoff),
            q: shared(tone.q()),
            voices: Vec::new(),
            effects,
            mixed: BufferArray::new(),
            played: BufferArray::new(),
            one_voice: BufferArray::new(),
        }
    }

    /// Plays with `tone` from now on: the filter of the notes sounding too, the envelopes of
    /// the notes to come.
    pub(crate) fn set_tone(&mut self, tone: Tone) {
        self.cutoff.set(tone.cutoff);
        self.q.set(tone.q());
        self.tone = tone;
    }

    /// `note`'s key goes down: a new voice, the note's voice still held let go first.
    pub(crate) fn press(&mut self, note: Note) {
        self.let_go(note);
        self.make_room();
        let controls = VoiceControls {
            gate: shared(0.0),
            cutoff: self.cutoff.clone(),
            q: self.q.clone(),
            attack: self.tone.attack,
            release: self.tone.release,
        };
        self.voices
            .push(Voice::new(self.preset, note, controls, self.sample_rate));
    }

    /// `note`'s key is let go.
    pub(crate) fn let_go(&mut self, note: Note) {
        self.voices
            .iter_mut()
            .filter(|voice| voice.note() == note)
            .for_each(Voice::let_go);
    }

    pub(crate) fn let_go_of_all(&mut self) {
        self.voices.iter_mut().for_each(Voice::let_go);
    }

    /// Whether a note of it sounds, held or going.
    pub(crate) fn has_voices(&self) -> bool {
        !self.voices.is_empty()
    }

    /// Its next [`CHUNK`] frames, the left side's and the right side's.
    pub(crate) fn play(&mut self) -> (&[f32], &[f32]) {
        self.mix_voices();
        self.voices.retain(|voice| !voice.is_over());
        self.effects.process(
            CHUNK,
            &self.mixed.buffer_ref(),
            &mut self.played.buffer_mut(),
        );
        let played = self.played.buffer_ref();
        (played.channel_f32(0), played.channel_f32(1))
    }

    /// Cuts the longest-going voice, a let-go one first, while as many play in full as may.
    fn make_room(&mut self) {
        let in_full = self.voices.iter().filter(|voice| !voice.is_cut()).count();
        if in_full < MOST_VOICES {
            return;
        }
        let longest_going = |held: bool| {
            self.voices
                .iter()
                .position(|voice| !voice.is_cut() && voice.is_held() == held)
        };
        if let Some(index) = longest_going(false).or_else(|| longest_going(true)) {
            self.voices[index].cut();
        }
    }

    fn mix_voices(&mut self) {
        let mix = self.mixed.channel_f32_mut(0);
        mix.fill(0.0);
        for voice in &mut self.voices {
            voice.play_into(mix, &mut self.one_voice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 48_000.0;

    fn organ() -> Layer {
        Layer::new(Preset::Organ, Preset::Organ.tone(), SAMPLE_RATE)
    }

    fn in_full(layer: &Layer) -> usize {
        layer.voices.iter().filter(|voice| !voice.is_cut()).count()
    }

    #[test]
    fn a_key_beyond_the_most_voices_cuts_the_longest_going_let_go_first() {
        let mut layer = organ();
        for number in 0..MOST_VOICES as u8 {
            layer.press(Note(48 + number));
        }
        layer.let_go(Note(50));
        layer.press(Note(80));
        assert_eq!(in_full(&layer), MOST_VOICES);
        assert!(layer.voices[2].is_cut(), "the let-go voice is cut first");
        layer.press(Note(81));
        assert!(layer.voices[0].is_cut(), "then the longest-held");
        assert_eq!(in_full(&layer), MOST_VOICES);
    }

    #[test]
    fn a_note_pressed_again_lets_its_first_voice_go() {
        let mut layer = organ();
        layer.press(Note(60));
        layer.press(Note(60));
        assert_eq!(layer.voices.len(), 2);
        assert!(!layer.voices[0].is_held());
        assert!(layer.voices[1].is_held());
    }

    #[test]
    fn voices_over_are_dropped_and_the_layer_falls_silent() {
        let mut layer = organ();
        layer.press(Note(60));
        layer.play();
        layer.let_go_of_all();
        for _ in 0..2_000 {
            layer.play();
        }
        assert!(!layer.has_voices());
        let (left, right) = layer.play();
        let loudest = left
            .iter()
            .chain(right)
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(loudest < 1e-3, "still at {loudest} long after the note");
    }
}
