//! The level a note's key runs: up as the key goes down, held while it is held, and down to
//! nothing once it is let go.
//!
//! Not fundsp's `adsr_live`, which waits for its control to be at rest before it takes a key's
//! press: a voice here is made with its key already down.

/// An envelope of attack, decay, sustain and release, which starts with its key down.
#[derive(Clone, Debug)]
pub(crate) struct KeyEnvelope {
    /// The seconds it takes to rise to full.
    attack: f32,
    /// The seconds it then takes to fall to `sustain`, where it stays while the key is held.
    decay: f32,
    sustain: f32,
    /// The seconds it takes to fall to nothing once the key is let go.
    release: f32,
    /// When the key was let go, and the level it had then.
    let_go: Option<(f32, f32)>,
}

impl KeyEnvelope {
    pub(crate) fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> KeyEnvelope {
        KeyEnvelope {
            attack,
            decay,
            sustain,
            release,
            let_go: None,
        }
    }

    /// The level `time` seconds after the key went down, the key `held` or not now. Asked at
    /// times that only go forward.
    pub(crate) fn level(&mut self, time: f32, held: bool) -> f32 {
        let (let_go_at, level_then) = match self.let_go {
            Some(let_go) => let_go,
            None if held => return self.held_level(time),
            None => *self.let_go.insert((time, self.held_level(time))),
        };
        let fallen = (time - let_go_at) / self.release;
        level_then * (1.0 - fallen).clamp(0.0, 1.0)
    }

    /// The level at `time` of a key still held.
    fn held_level(&self, time: f32) -> f32 {
        if time < self.attack {
            return time / self.attack;
        }
        let decayed = ((time - self.attack) / self.decay).min(1.0);
        1.0 + (self.sustain - 1.0) * decayed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_key_rises_to_full_then_falls_to_its_sustain() {
        let mut envelope = KeyEnvelope::new(0.1, 0.2, 0.5, 1.0);
        assert_eq!(envelope.level(0.0, true), 0.0);
        assert_eq!(envelope.level(0.05, true), 0.5);
        assert_eq!(envelope.level(0.1, true), 1.0);
        assert_eq!(envelope.level(0.2, true), 0.75);
        assert_eq!(envelope.level(5.0, true), 0.5);
    }

    #[test]
    fn a_key_let_go_falls_to_nothing_from_where_it_was() {
        let mut envelope = KeyEnvelope::new(0.1, 0.2, 0.5, 1.0);
        assert_eq!(envelope.level(0.05, true), 0.5);
        assert_eq!(envelope.level(0.05, false), 0.5);
        assert_eq!(envelope.level(0.55, false), 0.25);
        assert!(envelope.level(1.05, false) < 1e-6);
        assert_eq!(envelope.level(9.0, false), 0.0);
    }

    #[test]
    fn a_key_pressed_again_does_not_bring_a_let_go_note_back() {
        let mut envelope = KeyEnvelope::new(0.1, 0.2, 0.5, 1.0);
        envelope.level(1.0, false);
        assert_eq!(envelope.level(3.0, true), 0.0);
    }
}
