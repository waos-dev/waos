//! How a preset sounds beyond its notes: what the controls set.

/// The settings every note of a preset is played with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tone {
    /// Where the filter starts to cut, in hertz.
    pub(crate) cutoff: f32,
    /// How much the filter rings at its cutoff, from 0 to 1.
    pub(crate) resonance: f32,
    /// How long a note takes to come in, in seconds.
    pub(crate) attack: f32,
    /// How long a note takes to go once its key is let go, in seconds.
    pub(crate) release: f32,
    /// How loud everything is, from 0 to 1.
    pub(crate) volume: f32,
}

/// The filter's Q with no resonance, just short of a flat response, and how many times that at
/// full resonance, short of where the filter rings by itself.
const FLATTEST_Q: f32 = 0.6;
const Q_SPAN: f32 = 15.0;

impl Tone {
    /// The filter's Q the resonance asks for: each step of the control rings as much more as the
    /// last.
    pub(crate) fn q(&self) -> f32 {
        FLATTEST_Q * Q_SPAN.powf(self.resonance.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resonance_runs_the_filter_from_flat_to_ringing() {
        let tone = |resonance| Tone {
            cutoff: 1000.0,
            resonance,
            attack: 0.01,
            release: 0.1,
            volume: 1.0,
        };
        assert_eq!(tone(0.0).q(), 0.6);
        assert!((tone(1.0).q() - 9.0).abs() < 1e-4);
        assert_eq!(tone(7.0).q(), tone(1.0).q());
    }
}
