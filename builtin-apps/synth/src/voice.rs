//! One note sounding: a preset's graph for the note, from its key going down until its release
//! has passed, or until it is cut short to make room for another.

use fundsp::prelude32::{AudioUnit, BufferArray, BufferRef, MAX_BUFFER_SIZE, Shared, U1};

use crate::pitch::Note;
use crate::presets::{Preset, VoiceControls};

/// The frames a voice is asked for at once: fundsp's largest buffer.
pub(crate) const CHUNK: usize = MAX_BUFFER_SIZE;

/// How long a voice cut short takes to fade, in seconds: short enough to make room at once,
/// long enough not to click.
const CUT_SECONDS: f64 = 0.006;

/// How long after its release a voice is kept, in seconds: an envelope is read every two
/// milliseconds or so, and reaches nothing that much late.
const ENVELOPE_LAG_SECONDS: f64 = 0.01;

pub(crate) struct Voice {
    note: Note,
    sound: Box<dyn AudioUnit>,
    /// Above zero while the note's key is held, which the sound's envelopes read.
    gate: Shared,
    /// The frames a release takes, with the envelope's lag.
    release_frames: usize,
    /// The frames a cut takes.
    cut_frames: usize,
    end: Option<End>,
}

/// How a voice is ending, and the frames it has left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// Its key was let go: its own envelope takes it to nothing.
    Released { left: usize },
    /// It was cut short: its level is taken to nothing here.
    Cut { left: usize },
}

impl Voice {
    /// A voice of `preset` for `note`, its key down, at `sample_rate` frames a second.
    pub(crate) fn new(
        preset: Preset,
        note: Note,
        controls: VoiceControls,
        sample_rate: f64,
    ) -> Voice {
        controls.gate.set(1.0);
        let mut sound = preset.voice(note.hertz(), &controls);
        sound.set_sample_rate(sample_rate);
        sound.allocate();
        Voice {
            note,
            sound,
            gate: controls.gate,
            release_frames: frames_in(
                f64::from(controls.release) + ENVELOPE_LAG_SECONDS,
                sample_rate,
            ),
            cut_frames: frames_in(CUT_SECONDS, sample_rate),
            end: None,
        }
    }

    pub(crate) fn note(&self) -> Note {
        self.note
    }

    /// Whether its key is still held.
    pub(crate) fn is_held(&self) -> bool {
        self.end.is_none()
    }

    /// Whether it was cut short.
    pub(crate) fn is_cut(&self) -> bool {
        matches!(self.end, Some(End::Cut { .. }))
    }

    /// Whether nothing is left of it.
    pub(crate) fn is_over(&self) -> bool {
        matches!(
            self.end,
            Some(End::Released { left: 0 } | End::Cut { left: 0 })
        )
    }

    /// Its key is let go: it plays its release, then is over.
    pub(crate) fn let_go(&mut self) {
        if self.is_held() {
            self.gate.set(0.0);
            self.end = Some(End::Released {
                left: self.release_frames,
            });
        }
    }

    /// Fades it out at once, wherever it is.
    pub(crate) fn cut(&mut self) {
        if !self.is_cut() {
            self.gate.set(0.0);
            self.end = Some(End::Cut {
                left: self.cut_frames,
            });
        }
    }

    /// Adds its next [`CHUNK`] frames to `mix`, through `scratch`.
    pub(crate) fn play_into(&mut self, mix: &mut [f32], scratch: &mut BufferArray<U1>) {
        self.sound
            .process(CHUNK, &BufferRef::empty(), &mut scratch.buffer_mut());
        let played = scratch.channel_f32(0);
        match &mut self.end {
            Some(End::Cut { left }) => add_fading(mix, played, left, self.cut_frames),
            Some(End::Released { left }) => {
                add(mix, played);
                *left = left.saturating_sub(CHUNK);
            }
            None => add(mix, played),
        }
    }
}

/// The whole frames `seconds` take at `sample_rate`, one at least.
fn frames_in(seconds: f64, sample_rate: f64) -> usize {
    ((seconds * sample_rate).ceil() as usize).max(1)
}

fn add(mix: &mut [f32], played: &[f32]) {
    for (mixed, sample) in mix.iter_mut().zip(played) {
        *mixed += sample;
    }
}

/// Adds `played` to `mix` at a level that falls a frame at a time, `left` of `whole` frames
/// from nothing.
fn add_fading(mix: &mut [f32], played: &[f32], left: &mut usize, whole: usize) {
    for (mixed, sample) in mix.iter_mut().zip(played) {
        *mixed += sample * (*left as f32 / whole as f32);
        *left = left.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use fundsp::prelude32::shared;

    use super::*;
    use crate::tone::Tone;

    const SAMPLE_RATE: f64 = 48_000.0;

    fn voice(preset: Preset, tone: Tone) -> Voice {
        let controls = VoiceControls {
            gate: shared(0.0),
            cutoff: shared(tone.cutoff),
            q: shared(tone.q()),
            attack: tone.attack,
            release: tone.release,
        };
        Voice::new(preset, Note(57), controls, SAMPLE_RATE)
    }

    fn next_chunk(voice: &mut Voice) -> Vec<f32> {
        let mut mix = vec![0.0; CHUNK];
        voice.play_into(&mut mix, &mut BufferArray::new());
        mix
    }

    fn loudest(samples: &[f32]) -> f32 {
        samples
            .iter()
            .fold(0.0, |peak, sample| peak.max(sample.abs()))
    }

    /// The largest step between two frames next to each other.
    fn steepest(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .fold(0.0, |step, pair| step.max((pair[1] - pair[0]).abs()))
    }

    #[test]
    fn every_preset_starts_from_nothing_sounds_and_ends_in_nothing() {
        for preset in Preset::ALL {
            let mut voice = voice(preset, preset.tone());
            let first = next_chunk(&mut voice);
            assert!(first[0].abs() < 1e-3, "{preset:?} starts at {}", first[0]);
            let held: Vec<f32> = (0..200).flat_map(|_| next_chunk(&mut voice)).collect();
            assert!(held.iter().all(|sample| sample.is_finite()), "{preset:?}");
            let peak = loudest(&held);
            assert!((0.05..0.5).contains(&peak), "{preset:?} peaks at {peak}");
            voice.let_go();
            while !voice.is_over() {
                next_chunk(&mut voice);
            }
            let after = loudest(&next_chunk(&mut voice));
            assert!(after < 1e-4, "{preset:?} still sounds at {after} once over");
        }
    }

    #[test]
    fn a_voice_cut_short_fades_to_nothing_without_a_step() {
        let mut voice = voice(Preset::Organ, Preset::Organ.tone());
        let before: Vec<f32> = (0..100).flat_map(|_| next_chunk(&mut voice)).collect();
        voice.cut();
        let mut fading = Vec::new();
        while !voice.is_over() {
            fading.extend(next_chunk(&mut voice));
        }
        assert!(fading.len() <= 512, "a cut takes {} frames", fading.len());
        assert_eq!(*fading.last().unwrap(), 0.0);
        assert!(steepest(&fading) <= steepest(&before) * 1.5);
    }
}
