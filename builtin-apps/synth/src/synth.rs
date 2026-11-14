//! The instrument's sound: the preset being played, the presets played before it ringing out,
//! and the volume, as frames of two channels ready for an output.
//!
//! The invariants: nothing changes by a step. A preset changed leaves the old one's notes and
//! effects to ring out under a fade; the volume moves a little each frame; and what is too loud
//! is bent under full scale, never clipped.

use crate::layer::Layer;
use crate::pitch::Note;
use crate::presets::Preset;
use crate::tone::Tone;
use crate::voice::CHUNK;

/// The channels of a frame: the left side and the right.
pub(crate) const CHANNELS: usize = 2;

/// How long a preset played before rings out, in seconds, and how long once another change
/// of preset hurries it: each preset ringing costs its effects, and only one rings at leisure.
const RING_OUT_SECONDS: f64 = 2.0;
const HURRIED_SECONDS: f64 = 0.03;

/// How much of the way to the volume asked for the volume moves each frame.
const VOLUME_STEP: f32 = 0.002;

/// The level below which a frame counts as silence, and how long silence lasts, in seconds,
/// before the synth is quiet: longer than any gap in a tail still ringing.
const SILENCE: f32 = 1e-4;
const QUIET_SECONDS: f64 = 0.3;

/// The level up to which sound passes as it is; above it, it is bent to stay under full scale.
const LIMIT_KNEE: f32 = 0.7;

pub(crate) struct Synth {
    sample_rate: f64,
    playing: Layer,
    ringing_out: Vec<RingingOut>,
    /// The gain the volume asks for, and the gain now, on its way there.
    gain_asked: f32,
    gain: f32,
    /// The frames of silence played since the last sound.
    silent_frames: usize,
}

/// A layer no longer played, the level its fade has fallen to, and how far it falls each frame.
struct RingingOut {
    layer: Layer,
    level: f32,
    fall: f32,
}

impl Synth {
    /// A synth playing `preset` with `tone`, no note sounding and so quiet, at `sample_rate`
    /// frames a second.
    pub(crate) fn new(sample_rate: f64, preset: Preset, tone: Tone) -> Synth {
        let gain = gain_of(tone.volume);
        Synth {
            sample_rate,
            playing: Layer::new(preset, tone, sample_rate),
            ringing_out: Vec::new(),
            gain_asked: gain,
            gain,
            silent_frames: (QUIET_SECONDS * sample_rate).ceil() as usize,
        }
    }

    pub(crate) fn press(&mut self, note: Note) {
        self.playing.press(note);
        self.silent_frames = 0;
    }

    pub(crate) fn let_go(&mut self, note: Note) {
        self.playing.let_go(note);
    }

    pub(crate) fn set_tone(&mut self, tone: Tone) {
        self.gain_asked = gain_of(tone.volume);
        self.playing.set_tone(tone);
    }

    /// Plays `preset` with `tone` from the next key on. What sounds now rings out, and what
    /// was ringing out already is hurried.
    pub(crate) fn change_preset(&mut self, preset: Preset, tone: Tone) {
        let next = Layer::new(preset, tone, self.sample_rate);
        let mut before = std::mem::replace(&mut self.playing, next);
        self.gain_asked = gain_of(tone.volume);
        if self.is_quiet() {
            return;
        }
        before.let_go_of_all();
        let hurried_frames = (HURRIED_SECONDS * self.sample_rate) as f32;
        for ringing in &mut self.ringing_out {
            ringing.fall = ringing.fall.max(ringing.level / hurried_frames);
        }
        self.ringing_out.push(RingingOut {
            layer: before,
            level: 1.0,
            fall: 1.0 / (RING_OUT_SECONDS * self.sample_rate) as f32,
        });
    }

    /// Whether nothing sounds and nothing has for a while: playing on would play silence.
    pub(crate) fn is_quiet(&self) -> bool {
        self.silent_frames as f64 >= QUIET_SECONDS * self.sample_rate
    }

    /// The next `chunks` chunks of [`CHUNK`] frames, each frame its left sample then its right.
    pub(crate) fn play(&mut self, chunks: usize) -> Vec<f32> {
        let mut frames = Vec::with_capacity(chunks * CHUNK * CHANNELS);
        for _ in 0..chunks {
            self.play_chunk(&mut frames);
        }
        frames
    }

    /// Appends the next chunk to `frames`.
    fn play_chunk(&mut self, frames: &mut Vec<f32>) {
        let (mut left, mut right) = ([0.0; CHUNK], [0.0; CHUNK]);
        let (played_left, played_right) = self.playing.play();
        left.copy_from_slice(&played_left[..CHUNK]);
        right.copy_from_slice(&played_right[..CHUNK]);
        for ringing in &mut self.ringing_out {
            ringing.play_into(&mut left, &mut right);
        }
        self.ringing_out.retain(|ringing| ringing.level > 0.0);
        let mut loudest = 0.0f32;
        for (left, right) in left.into_iter().zip(right) {
            self.gain += (self.gain_asked - self.gain) * VOLUME_STEP;
            let frame = [limited(left * self.gain), limited(right * self.gain)];
            loudest = loudest.max(frame[0].abs()).max(frame[1].abs());
            frames.extend(frame);
        }
        self.count_silence(loudest);
    }

    fn count_silence(&mut self, loudest: f32) {
        if loudest < SILENCE && !self.playing.has_voices() {
            self.silent_frames += CHUNK;
        } else {
            self.silent_frames = 0;
        }
    }
}

impl RingingOut {
    /// Adds its next chunk to `left` and `right`, at the level its fade falls through.
    fn play_into(&mut self, left: &mut [f32; CHUNK], right: &mut [f32; CHUNK]) {
        let (played_left, played_right) = self.layer.play();
        for index in 0..CHUNK {
            left[index] += played_left[index] * self.level;
            right[index] += played_right[index] * self.level;
            self.level = (self.level - self.fall).max(0.0);
        }
    }
}

/// The gain a volume from 0 to 1 asks for: its square, which the ear hears as even steps.
fn gain_of(volume: f32) -> f32 {
    let volume = volume.clamp(0.0, 1.0);
    volume * volume
}

/// `sample` as it is up to the knee, and bent above it so that it never reaches full scale.
fn limited(sample: f32) -> f32 {
    let level = sample.abs();
    if level <= LIMIT_KNEE {
        return sample;
    }
    let room = 1.0 - LIMIT_KNEE;
    let bent = LIMIT_KNEE + room * ((level - LIMIT_KNEE) / room).tanh();
    bent.copysign(sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 48_000.0;

    fn synth(preset: Preset) -> Synth {
        Synth::new(SAMPLE_RATE, preset, preset.tone())
    }

    fn loudest(frames: &[f32]) -> f32 {
        frames
            .iter()
            .fold(0.0, |peak, sample| peak.max(sample.abs()))
    }

    /// The largest step between two frames next to each other, on either side.
    fn steepest(frames: &[f32]) -> f32 {
        frames.windows(CHANNELS + 1).fold(0.0, |step, frame| {
            step.max((frame[CHANNELS] - frame[0]).abs())
        })
    }

    #[test]
    fn a_new_synth_is_quiet_and_plays_silence() {
        let mut synth = synth(Preset::WarmPad);
        assert!(synth.is_quiet());
        let frames = synth.play(3);
        assert_eq!(frames.len(), 3 * CHUNK * CHANNELS);
        assert_eq!(loudest(&frames), 0.0);
    }

    #[test]
    fn a_key_sounds_on_both_sides_and_its_note_dies_away_into_quiet() {
        for preset in Preset::ALL {
            let mut synth = synth(preset);
            synth.press(Note(60));
            assert!(!synth.is_quiet());
            let held = synth.play(200);
            let (left, right): (Vec<f32>, Vec<f32>) = held
                .chunks(CHANNELS)
                .map(|frame| (frame[0], frame[1]))
                .unzip();
            assert!(
                loudest(&left) > 0.01 && loudest(&right) > 0.01,
                "{preset:?}"
            );
            synth.let_go(Note(60));
            let mut chunks = 0;
            while !synth.is_quiet() {
                synth.play(1);
                chunks += 1;
                assert!(chunks < 20 * 750, "{preset:?} never falls quiet");
            }
        }
    }

    #[test]
    fn ten_keys_at_once_stay_under_full_scale() {
        for preset in Preset::ALL {
            let mut synth = synth(preset);
            synth.set_tone(Tone {
                volume: 1.0,
                ..preset.tone()
            });
            for number in 0..10 {
                synth.press(Note(48 + number * 3));
            }
            let peak = loudest(&synth.play(400));
            assert!(peak < 1.0, "{preset:?} reaches {peak}");
        }
    }

    #[test]
    fn a_preset_changed_under_held_keys_makes_no_step() {
        let mut synth = synth(Preset::Organ);
        synth.press(Note(57));
        synth.press(Note(64));
        let before = synth.play(100);
        synth.change_preset(Preset::Bass, Preset::Bass.tone());
        let after = synth.play(100);
        assert!(loudest(&after) > 0.01, "the old preset rings out");
        let across: Vec<f32> = before[before.len() - 8..]
            .iter()
            .chain(&after[..8])
            .copied()
            .collect();
        assert!(steepest(&across) <= steepest(&before) * 1.5);
    }

    #[test]
    fn a_preset_changed_in_silence_leaves_nothing_ringing() {
        let mut synth = synth(Preset::Organ);
        synth.change_preset(Preset::Bass, Preset::Bass.tone());
        assert!(synth.ringing_out.is_empty());
        assert!(synth.is_quiet());
    }

    #[test]
    fn a_second_change_of_preset_hurries_the_first_out_without_a_step() {
        let mut synth = synth(Preset::Organ);
        synth.press(Note(57));
        synth.play(100);
        synth.change_preset(Preset::Bass, Preset::Bass.tone());
        let before = synth.play(10);
        synth.change_preset(Preset::Lead, Preset::Lead.tone());
        let after = synth.play(40);
        assert_eq!(
            synth.ringing_out.len(),
            1,
            "the organ is gone within the 30 ms"
        );
        let across: Vec<f32> = before[before.len() - 8..]
            .iter()
            .chain(&after)
            .copied()
            .collect();
        assert!(steepest(&across) <= steepest(&before) * 3.0);
    }

    #[test]
    fn the_limit_leaves_quiet_sound_alone_and_keeps_loud_sound_under_full_scale() {
        assert_eq!(limited(0.5), 0.5);
        assert_eq!(limited(-0.7), -0.7);
        assert!(limited(0.9) < 0.9 && limited(0.9) > 0.7);
        assert!(limited(1.0) < 1.0);
        assert!(limited(30.0) <= 1.0 && limited(-30.0) >= -1.0);
    }
}
