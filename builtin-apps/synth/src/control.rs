//! The controls of the tone: what each sets, the range its slider runs over, and how its value
//! is shown.

use crate::tone::Tone;

/// A setting of the tone its user moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Control {
    Cutoff,
    Resonance,
    Attack,
    Release,
    Volume,
}

/// How a slider's travel maps to values between `lowest` and `highest`.
#[derive(Clone, Copy)]
enum Travel {
    /// Each step adds as much: for what is heard evenly.
    Even { lowest: f32, highest: f32 },
    /// Each step multiplies by as much: for pitches and times, which are heard by their ratio.
    ByRatio { lowest: f32, highest: f32 },
}

impl Control {
    pub(crate) const ALL: [Control; 5] = [
        Control::Cutoff,
        Control::Resonance,
        Control::Attack,
        Control::Release,
        Control::Volume,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Control::Cutoff => "Cutoff",
            Control::Resonance => "Resonance",
            Control::Attack => "Attack",
            Control::Release => "Release",
            Control::Volume => "Volume",
        }
    }

    /// Its value in `tone`.
    pub(crate) fn of(self, tone: &Tone) -> f32 {
        match self {
            Control::Cutoff => tone.cutoff,
            Control::Resonance => tone.resonance,
            Control::Attack => tone.attack,
            Control::Release => tone.release,
            Control::Volume => tone.volume,
        }
    }

    /// `tone` with it at `value`.
    pub(crate) fn set(self, tone: Tone, value: f32) -> Tone {
        match self {
            Control::Cutoff => Tone {
                cutoff: value,
                ..tone
            },
            Control::Resonance => Tone {
                resonance: value,
                ..tone
            },
            Control::Attack => Tone {
                attack: value,
                ..tone
            },
            Control::Release => Tone {
                release: value,
                ..tone
            },
            Control::Volume => Tone {
                volume: value,
                ..tone
            },
        }
    }

    /// Where its slider stands for `value`, from 0 to 1.
    pub(crate) fn position(self, value: f32) -> f64 {
        let position = match self.travel() {
            Travel::Even { lowest, highest } => (value - lowest) / (highest - lowest),
            Travel::ByRatio { lowest, highest } => (value / lowest).ln() / (highest / lowest).ln(),
        };
        f64::from(position.clamp(0.0, 1.0))
    }

    /// Its value with its slider at `position`, from 0 to 1.
    pub(crate) fn value_at(self, position: f64) -> f32 {
        let position = position.clamp(0.0, 1.0) as f32;
        match self.travel() {
            Travel::Even { lowest, highest } => lowest + (highest - lowest) * position,
            Travel::ByRatio { lowest, highest } => lowest * (highest / lowest).powf(position),
        }
    }

    /// `value` as its user reads it: `2.2 kHz`, `350 ms`, `80%`.
    pub(crate) fn shown(self, value: f32) -> String {
        match self {
            Control::Cutoff => hertz(value),
            Control::Attack | Control::Release => time(value),
            Control::Resonance | Control::Volume => format!("{:.0}%", value * 100.0),
        }
    }

    fn travel(self) -> Travel {
        match self {
            Control::Cutoff => Travel::ByRatio {
                lowest: 80.0,
                highest: 12_000.0,
            },
            Control::Attack => Travel::ByRatio {
                lowest: 0.003,
                highest: 2.0,
            },
            Control::Release => Travel::ByRatio {
                lowest: 0.03,
                highest: 4.0,
            },
            Control::Resonance | Control::Volume => Travel::Even {
                lowest: 0.0,
                highest: 1.0,
            },
        }
    }
}

fn hertz(hertz: f32) -> String {
    if hertz < 1_000.0 {
        format!("{hertz:.0} Hz")
    } else {
        format!("{:.1} kHz", hertz / 1_000.0)
    }
}

fn time(seconds: f32) -> String {
    if seconds < 1.0 {
        format!("{:.0} ms", seconds * 1_000.0)
    } else {
        format!("{seconds:.1} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::Preset;

    #[test]
    fn a_slider_moved_to_where_a_value_stands_gives_the_value_back() {
        for preset in Preset::ALL {
            let tone = preset.tone();
            for control in Control::ALL {
                let value = control.of(&tone);
                let back = control.value_at(control.position(value));
                assert!(
                    (back - value).abs() <= value.abs() * 1e-3,
                    "{control:?} of {preset:?}"
                );
            }
        }
    }

    #[test]
    fn the_cutoff_slider_runs_by_ratio_from_80_hertz_to_12_kilohertz() {
        assert_eq!(Control::Cutoff.value_at(0.0), 80.0);
        assert!((Control::Cutoff.value_at(1.0) - 12_000.0).abs() < 1.0);
        let middle = Control::Cutoff.value_at(0.5);
        assert!((middle - (80.0f32 * 12_000.0).sqrt()).abs() < 1.0);
        assert_eq!(Control::Cutoff.position(30.0), 0.0);
    }

    #[test]
    fn a_control_moved_changes_only_its_own_setting() {
        let tone = Preset::Bass.tone();
        let moved = Control::Attack.set(tone, 0.5);
        assert_eq!(
            moved,
            Tone {
                attack: 0.5,
                ..tone
            }
        );
    }

    #[test]
    fn values_are_shown_in_the_units_they_are_heard_in() {
        assert_eq!(Control::Cutoff.shown(850.0), "850 Hz");
        assert_eq!(Control::Cutoff.shown(2_200.0), "2.2 kHz");
        assert_eq!(Control::Attack.shown(0.003), "3 ms");
        assert_eq!(Control::Release.shown(1.4), "1.4 s");
        assert_eq!(Control::Volume.shown(0.8), "80%");
    }
}
