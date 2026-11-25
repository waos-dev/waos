//! What of a context's sound keeps its program unfrozen: the context is audible while it runs with
//! sound playing now, a source whose start its clock has reached and which has not ended, or a
//! stream input open. web-audio-api reads no output level, so this is read against the context's
//! clock at each tick of the engine's time, and each time a source is scheduled or ends or the
//! context is suspended or resumed: a source scheduled far ahead keeps nothing unfrozen until it
//! starts. Recording keeps the program unfrozen on its own, through `microphone`.
//!
//! The invariant: the program holds one `Sounding` for the context while it is audible and none
//! otherwise, decided under the context's lock each time the sound is read; the `Sounding` is let
//! go after the lock, since letting it go keeps the freeze rule.

use std::sync::{Arc, Mutex, Weak};

use crate::engine_time::{EngineClock, Timer};
use crate::lifecycle::{FreezeInhibitor, FreezeInhibitorKind, Lifecycle};
use crate::sync::locked;

/// One context's sound, as its program's freeze weighs it.
pub(super) struct ContextSound {
    lifecycle: Lifecycle,
    /// The context's clock: its current time, in seconds.
    clock: Box<dyn Fn() -> f64 + Send + Sync>,
    state: Mutex<SoundState>,
    /// What reads the sound at each tick of the engine's time; the engine's clock holds it only
    /// while the sound is held.
    _read_at_each_tick: Arc<Timer>,
}

struct SoundState {
    running: bool,
    /// When each source scheduled and not ended starts, on the context's clock.
    starts: Vec<f64>,
    /// Held while the context is audible.
    freeze_inhibitor: Option<FreezeInhibitor>,
}

impl SoundState {
    /// Whether the context is audible with its clock at `now`.
    fn audible(&self, now: f64) -> bool {
        self.running && self.starts.iter().any(|start| *start <= now)
    }
}

impl ContextSound {
    /// A context that runs, as one does once made, playing nothing yet, of the program whose
    /// lifecycle is `lifecycle`, read at each tick of `engine_clock` against `clock`, the
    /// context's own.
    pub(super) fn new(
        lifecycle: Lifecycle,
        engine_clock: &EngineClock,
        clock: impl Fn() -> f64 + Send + Sync + 'static,
    ) -> Arc<ContextSound> {
        let sound = Arc::new_cyclic(|sound: &Weak<ContextSound>| {
            let read_sound = Weak::clone(sound);
            let read_at_each_tick: Arc<Timer> = Arc::new(move |_| {
                if let Some(sound) = read_sound.upgrade() {
                    sound.read();
                }
            });
            ContextSound {
                lifecycle,
                clock: Box::new(clock),
                state: Mutex::new(SoundState {
                    running: true,
                    starts: Vec::new(),
                    freeze_inhibitor: None,
                }),
                _read_at_each_tick: read_at_each_tick,
            }
        });
        engine_clock.at_each_tick(&sound._read_at_each_tick);
        sound
    }

    /// The context runs, or no longer does: suspended or closed.
    pub(super) fn set_running(&self, running: bool) {
        self.change(|state| state.running = running);
    }

    /// A source plays from `start`, on the context's clock, until the answer is dropped.
    pub(super) fn play_from(self: &Arc<Self>, start: f64) -> Playing {
        self.change(|state| state.starts.push(start));
        Playing {
            sound: Arc::clone(self),
            start,
        }
    }

    /// A source plays from now until the answer is dropped: a stream input while it is open.
    pub(super) fn play(self: &Arc<Self>) -> Playing {
        self.play_from(f64::NEG_INFINITY)
    }

    /// Reads the sound against the context's clock as it stands.
    fn read(&self) {
        self.change(|_| {});
    }

    /// Changes what the sound depends on, and holds the program unfrozen while the context is
    /// audible.
    fn change(&self, change: impl FnOnce(&mut SoundState)) {
        let released = {
            let mut state = locked(&self.state);
            change(&mut state);
            if state.audible((self.clock)()) {
                if state.freeze_inhibitor.is_none() {
                    state.freeze_inhibitor = Some(
                        self.lifecycle
                            .inhibit_freeze(FreezeInhibitorKind::AudioPlayback),
                    );
                }
                None
            } else {
                state.freeze_inhibitor.take()
            }
        };
        drop(released);
    }
}

/// A source scheduled on a context, held from its start until it ends.
pub(super) struct Playing {
    sound: Arc<ContextSound>,
    start: f64,
}

impl Drop for Playing {
    fn drop(&mut self) {
        let start = self.start.to_bits();
        self.sound.change(|state| {
            if let Some(at) = state.starts.iter().position(|s| s.to_bits() == start) {
                state.starts.swap_remove(at);
            }
        });
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::engine::engine_config;
    use crate::engine_time::{EngineTime, ManualTime};
    use crate::epoch::TICK;
    use crate::lifecycle::FreezePolicy;
    use crate::running::Programs;

    /// A program out of sight, frozen at once, on the engine's time a test moves.
    pub(in crate::audio) struct PausedProgram {
        pub(in crate::audio) time: ManualTime,
        pub(in crate::audio) clock: EngineClock,
        pub(in crate::audio) lifecycle: Lifecycle,
        /// What the time ticks, which its ticks hold weakly.
        _engine: Arc<wasmtime::Engine>,
    }

    impl PausedProgram {
        pub(in crate::audio) fn new() -> PausedProgram {
            let time = ManualTime::new();
            let engine = Arc::new(wasmtime::Engine::new(&engine_config(false)).unwrap());
            let clock = EngineTime::Manual(time.clone())
                .start(&engine, &Programs::default())
                .expect("one engine for this time");
            let lifecycle = Lifecycle::new(FreezePolicy::default(), clock.clone());
            lifecycle.app_suspending();
            PausedProgram {
                time,
                clock,
                lifecycle,
                _engine: engine,
            }
        }
    }

    /// A context's clock a test sets, at 0 until it does.
    fn context_clock() -> (Arc<Mutex<f64>>, impl Fn() -> f64 + Send + Sync + 'static) {
        let now = Arc::new(Mutex::new(0.0));
        let read = Arc::clone(&now);
        (now, move || *read.lock().unwrap())
    }

    #[test]
    fn a_running_context_with_a_source_playing_keeps_its_program_unfrozen() {
        let PausedProgram {
            clock, lifecycle, ..
        } = &PausedProgram::new();
        let sound = ContextSound::new(lifecycle.clone(), clock, || 0.0);
        assert!(lifecycle.is_frozen(), "running, playing nothing");
        let playing = sound.play();
        assert!(!lifecycle.is_frozen());
        drop(playing);
        assert!(lifecycle.is_frozen(), "the source ended");
    }

    #[test]
    fn a_suspended_context_is_silent_whatever_it_plays() {
        let PausedProgram {
            clock, lifecycle, ..
        } = &PausedProgram::new();
        let sound = ContextSound::new(lifecycle.clone(), clock, || 0.0);
        let _playing = sound.play();
        sound.set_running(false);
        assert!(lifecycle.is_frozen());
        sound.set_running(true);
        assert!(!lifecycle.is_frozen(), "resumed, it is audible again");
    }

    #[test]
    fn a_source_scheduled_ahead_is_silent_until_a_tick_finds_the_context_s_clock_at_its_start() {
        let PausedProgram {
            time,
            clock,
            lifecycle,
            ..
        } = &PausedProgram::new();
        let (now, context_clock) = context_clock();
        let sound = ContextSound::new(lifecycle.clone(), clock, context_clock);
        let playing = sound.play_from(1.0);
        time.elapse(TICK);
        assert!(lifecycle.is_frozen(), "scheduled ahead, it is silent");

        *now.lock().unwrap() = 1.0;
        assert!(lifecycle.is_frozen(), "read at the next tick");
        time.elapse(TICK);
        assert!(!lifecycle.is_frozen(), "playing now");

        drop(playing);
        assert!(lifecycle.is_frozen(), "the source ended");
    }

    #[test]
    fn a_source_ended_leaves_the_others_sounding() {
        let PausedProgram {
            clock, lifecycle, ..
        } = &PausedProgram::new();
        let sound = ContextSound::new(lifecycle.clone(), clock, || 2.0);
        let first = sound.play_from(1.0);
        let second = sound.play_from(1.0);
        let later = sound.play_from(3.0);
        drop(first);
        assert!(
            !lifecycle.is_frozen(),
            "one started at the same time still plays"
        );
        drop(second);
        assert!(lifecycle.is_frozen(), "the one left starts later");
        drop(later);
    }
}
