//! The presets: each the way a note of it sounds, the effects its notes play through, and the
//! tone its controls start at. A note's sound is a graph of fundsp's nodes made for that note,
//! at its pitch, which reads the note's key and the filter's settings as it plays.
//!
//! The invariant: every voice ends in its amplitude envelope, which rises from nothing and
//! falls to nothing, so a note never starts or stops with a click, and is silent once its
//! release has passed.

use fundsp::prelude32::*;

use crate::key_envelope::KeyEnvelope;
use crate::tone::Tone;

/// A sound the instrument plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preset {
    WarmPad,
    ElectricPiano,
    Pluck,
    Bass,
    Lead,
    Organ,
}

/// What a voice's graph reads as it plays.
pub(crate) struct VoiceControls {
    /// Above zero while the note's key is held.
    pub(crate) gate: Shared,
    /// The filter's cutoff, in hertz, and its Q.
    pub(crate) cutoff: Shared,
    pub(crate) q: Shared,
    /// How long the note takes to come in and to go, in seconds.
    pub(crate) attack: f32,
    pub(crate) release: f32,
}

/// The shortest an envelope's stage is: shorter, a stage is a step, which clicks.
const SHORTEST_STAGE: f32 = 0.003;

/// How long a change of the filter's controls takes to arrive, so that a slider dragged makes
/// no zipper noise.
const CONTROL_SMOOTHING: f32 = 0.03;

/// The range the filter's cutoff is kept in, whatever its envelope asks: the filter is
/// unstable near half the sample rate.
const LOWEST_CUTOFF: f32 = 30.0;
const HIGHEST_CUTOFF: f32 = 16_000.0;

impl Preset {
    pub(crate) const ALL: [Preset; 6] = [
        Preset::WarmPad,
        Preset::ElectricPiano,
        Preset::Pluck,
        Preset::Bass,
        Preset::Lead,
        Preset::Organ,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Preset::WarmPad => "Warm Pad",
            Preset::ElectricPiano => "Electric Piano",
            Preset::Pluck => "Pluck",
            Preset::Bass => "Bass",
            Preset::Lead => "Lead",
            Preset::Organ => "Organ",
        }
    }

    /// The tone its controls start at.
    pub(crate) fn tone(self) -> Tone {
        let (cutoff, resonance, attack, release) = match self {
            Preset::WarmPad => (2_200.0, 0.15, 0.35, 1.4),
            Preset::ElectricPiano => (5_000.0, 0.05, 0.003, 0.35),
            Preset::Pluck => (6_000.0, 0.1, 0.003, 0.6),
            Preset::Bass => (1_200.0, 0.45, 0.003, 0.12),
            Preset::Lead => (3_500.0, 0.3, 0.01, 0.25),
            Preset::Organ => (5_000.0, 0.0, 0.008, 0.06),
        };
        Tone {
            cutoff,
            resonance,
            attack,
            release,
            volume: 0.8,
        }
    }

    /// The sound of one note at `hertz`: no input, one output.
    pub(crate) fn voice(self, hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
        match self {
            Preset::WarmPad => warm_pad(hertz, controls),
            Preset::ElectricPiano => electric_piano(hertz, controls),
            Preset::Pluck => plucked_string(hertz, controls),
            Preset::Bass => bass(hertz, controls),
            Preset::Lead => lead(hertz, controls),
            Preset::Organ => organ_pipes(hertz, controls),
        }
    }

    /// What its notes, mixed, play through: one input, and two outputs, the left side and the
    /// right.
    pub(crate) fn effects(self) -> Box<dyn AudioUnit> {
        let both_sides = split::<U2>();
        match self {
            Preset::WarmPad => Box::new(both_sides >> wide_chorus(0.4) >> room(0.3, 3.0)),
            Preset::ElectricPiano => Box::new(both_sides >> wide_chorus(0.15) >> room(0.12, 1.5)),
            Preset::Pluck => Box::new(both_sides >> room(0.18, 2.0)),
            Preset::Bass => Box::new(both_sides),
            Preset::Lead => Box::new(both_sides >> echo(0.25) >> room(0.1, 1.5)),
            Preset::Organ => Box::new(both_sides >> wide_chorus(0.25) >> room(0.1, 1.2)),
        }
    }
}

/// Three saws a little apart in pitch, one an octave down, under a filter that opens slowly.
fn warm_pad(hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
    let saws = saw_hz(hertz * 0.996) + saw_hz(hertz * 1.004) + 0.6 * saw_hz(hertz * 0.5);
    let opening = 0.35 + 0.65 * envelope(controls, controls.attack * 1.5, 1.0, 0.7);
    let level = envelope(controls, controls.attack, 0.4, 0.85);
    Box::new(filtered(saws * 0.16, opening, controls) * level)
}

/// How far above the note the electric piano's tine rings, and the highest it rings, in hertz,
/// short of where it would fold back down.
const TINE_RATIO: f32 = 14.0;
const HIGHEST_TINE: f32 = 12_000.0;

/// A sine whose pitch a sine at its own pitch bends, more at the strike than after, with a
/// short bright tine at the strike: two-operator FM, as an electric piano's patch is.
fn electric_piano(hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
    let body = sine_hz(hertz) * (0.25 + 1.1 * envelope(controls, 0.0, 0.9, 0.0));
    let tine = sine_hz((hertz * TINE_RATIO).min(HIGHEST_TINE))
        * (4.0 * envelope(controls, 0.0, 0.06, 0.0));
    let tone = (dc(hertz) * (1.0 + body + tine)) >> sine();
    let level = envelope(controls, controls.attack, 2.5, 0.2);
    Box::new(filtered(tone * 0.15, dc(1.0), controls) * level)
}

/// A string plucked, Karplus and Strong's, which rings down by itself while its key is held.
fn plucked_string(hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
    let string = zero() >> pluck(hertz, 0.35, 0.4);
    let opening = 0.5 + 0.5 * envelope(controls, 0.0, 0.5, 0.0);
    let level = envelope(controls, controls.attack, 0.1, 1.0);
    Box::new(filtered(string * 0.32, opening, controls) * level)
}

/// A saw over a square an octave down, under a filter that snaps shut after the strike.
fn bass(hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
    let waves = saw_hz(hertz) + 0.8 * square_hz(hertz * 0.5);
    let opening = 0.25 + 0.75 * envelope(controls, 0.0, 0.18, 0.2);
    let level = envelope(controls, controls.attack, 0.3, 0.8);
    Box::new(filtered(waves * 0.14, opening, controls) * level)
}

/// A narrow pulse with a vibrato that comes in after the note has, beside a saw.
fn lead(hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
    let vibrato = sine_hz(5.5) * (0.006 * envelope(controls, 0.5, 0.1, 1.0));
    let pitch = dc(hertz) * (1.0 + vibrato);
    let waves = ((pitch | dc(0.3)) >> pulse()) + 0.5 * saw_hz(hertz * 1.003);
    let opening = 0.5 + 0.5 * envelope(controls, 0.01, 0.3, 0.6);
    let level = envelope(controls, controls.attack, 0.2, 0.85);
    Box::new(filtered(waves * 0.13, opening, controls) * level)
}

/// An organ's stops: the note, the octave above and the octave below, at full level while the
/// key is held.
fn organ_pipes(hertz: f32, controls: &VoiceControls) -> Box<dyn AudioUnit> {
    let stops = organ_hz(hertz) + 0.5 * organ_hz(hertz * 2.0) + 0.5 * sine_hz(hertz * 0.5);
    let level = envelope(controls, controls.attack, 0.01, 1.0);
    Box::new(filtered(stops * 0.11, dc(1.0), controls) * level)
}

/// An envelope the note's key runs: up to full in `attack` seconds, down to `sustain` in
/// `decay`, held there while the key is, and down to nothing in the note's release.
fn envelope(
    controls: &VoiceControls,
    attack: f32,
    decay: f32,
    sustain: f32,
) -> An<impl AudioNode<Inputs = U0, Outputs = U1> + use<>> {
    let mut envelope = KeyEnvelope::new(
        attack.max(SHORTEST_STAGE),
        decay.max(SHORTEST_STAGE),
        sustain,
        controls.release.max(SHORTEST_STAGE),
    );
    var(&controls.gate) >> envelope2(move |time, gate| envelope.level(time, gate > 0.0))
}

/// `source` through the lowpass filter the controls set, its cutoff scaled by `opening`, which
/// runs from 0 to 1.
fn filtered<Source, Opening>(
    source: An<Source>,
    opening: An<Opening>,
    controls: &VoiceControls,
) -> An<impl AudioNode<Inputs = U0, Outputs = U1> + use<Source, Opening>>
where
    Source: AudioNode<Inputs = U0, Outputs = U1>,
    Opening: AudioNode<Inputs = U0, Outputs = U1>,
{
    let asked = var(&controls.cutoff) >> follow(CONTROL_SMOOTHING);
    let cutoff = (asked * opening) >> clip_to(LOWEST_CUTOFF, HIGHEST_CUTOFF);
    let q = var(&controls.q) >> follow(CONTROL_SMOOTHING);
    (source | cutoff | q) >> lowpass()
}

/// Each side beside `amount` of a chorus of it, the two sides' choruses moving apart.
fn wide_chorus(amount: f32) -> An<impl AudioNode<Inputs = U2, Outputs = U2>> {
    let dry = 1.0 - amount * 0.5;
    let side = |seed| (dry * pass()) & (amount * chorus(seed, 0.0, 0.012, 0.25));
    side(0) | side(1)
}

/// Both sides beside `amount` of their reverberation in a room that rings for `seconds`.
fn room(amount: f32, seconds: f32) -> An<impl AudioNode<Inputs = U2, Outputs = U2>> {
    multipass::<U2>() & (amount * reverb_stereo(14.0, seconds, 0.6))
}

/// Each side beside `amount` of its echoes, the right side's a little later than the left's.
fn echo(amount: f32) -> An<impl AudioNode<Inputs = U2, Outputs = U2>> {
    let echoes = |seconds: f32| pass() & (amount * feedback(delay(seconds) * 0.45));
    echoes(0.30) | echoes(0.33)
}
