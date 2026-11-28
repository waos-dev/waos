//! The tick: at each 100 ms of the engine's time (`engine_time`), the clock's timers run (a view's
//! spare hardware buffers go, a program's sound is read), every program due a freeze by the clock
//! is frozen, and the epoch moves on, which interrupts every running program once, so that what a
//! store can only tell the engine from the inside, its fuel, reaches the program's stats, and so
//! that a freeze takes. A freeze takes at a tick whatever times it, so the tick times the delay
//! too, with nothing to cancel. The timers run before the freezes, since what they read, as a
//! program's sound starting, may keep a program from its freeze. With real time the engine's own
//! thread ticks; with time a test moves, the move runs each tick it passes.
//!
//! The invariant: a tick holds the engine weakly, so neither the thread nor a test's time keeps
//! it alive, and the thread ends with it.

use std::sync::{Arc, Weak};
use std::thread;
use std::time::Duration;

use wasmtime::Engine;

use crate::engine_time::EngineClock;
use crate::running::Programs;

/// How often every program is interrupted for its fuel to be read, and how late a freeze may
/// take: often enough for a panel of what programs cost, rare enough to cost nothing.
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// What a tick moves: the engine whose epoch it moves, the programs whose freezes it keeps, and
/// the clock they read, whose timers it runs.
pub(crate) struct Ticks {
    engine: Weak<Engine>,
    programs: Programs,
    clock: EngineClock,
}

impl Ticks {
    pub(crate) fn new(engine: &Arc<Engine>, programs: Programs, clock: EngineClock) -> Ticks {
        Ticks {
            engine: Arc::downgrade(engine),
            programs,
            clock,
        }
    }

    /// One tick, at the clock's time now: the timers, freezes that came due, then the epoch.
    /// Answers false once the engine is gone, and does nothing then.
    pub(crate) fn run(&self) -> bool {
        let Some(engine) = self.engine.upgrade() else {
            return false;
        };
        let now = self.clock.now();
        self.clock.run_timers(now);
        self.programs.freeze_if_due(now);
        engine.increment_epoch();
        true
    }
}

/// Runs `ticks` every [`TICK`] of real time on a thread of their own, until the engine is
/// dropped.
pub(crate) fn tick_in_real_time(ticks: Ticks) {
    thread::Builder::new()
        .name("epoch tick".into())
        .spawn(move || {
            loop {
                #[allow(
                    clippy::disallowed_methods,
                    reason = "the epoch ticks every TICK of real time"
                )]
                thread::sleep(TICK);
                if !ticks.run() {
                    return;
                }
            }
        })
        .expect("a thread for the epoch tick");
}
