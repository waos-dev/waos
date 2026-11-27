//! The engine's time: one clock that what freezes a program, when a view was hidden or shown, when
//! a view's spare hardware buffers go and when a program's sound is read, all read; and what moves
//! it, which runs the tick at each 100 ms it passes (`epoch`). The engine's own thread moves real
//! time ([`EngineTime::Real`]); a test moves time it keeps ([`EngineTime::Manual`]), as Waft's
//! tests move their `AppCell`'s clock and Flutter's `FakeAsync.elapse` moves its own. A program's
//! own clock (`clock`), which its `wasi:clocks` and its sleeps are on, is real time either way.
//!
//! The invariants a later editor will break: nothing the engine decides by time reads the
//! system's clock but through [`EngineClock`], so a test that moves time moves all of it; and the
//! ticks a move passes run in order, each with the clock at its own moment, as the thread would
//! have run them.

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use wasmtime::Engine;

use crate::epoch::{self, TICK, Ticks};
use crate::running::Programs;
use crate::sync::locked;

/// What moves the engine's time.
#[derive(Clone, Debug)]
pub enum EngineTime {
    /// The engine's own thread, every 100 ms: how an app runs.
    Real,
    /// A test, through the handle it keeps: nothing moves until it says.
    Manual(ManualTime),
}

impl EngineTime {
    /// The clock of an engine on this time, whose ticks move `engine`'s epoch and `programs`'
    /// freezes from now on: on a thread of their own, or as the test moves the time. A test's
    /// time moves one engine; another engine made with it is refused.
    pub(crate) fn start(&self, engine: &Arc<Engine>, programs: &Programs) -> Result<EngineClock> {
        let clock = match self {
            EngineTime::Real => EngineClock::real(),
            EngineTime::Manual(time) => time.clock(),
        };
        let ticks = Ticks::new(engine, programs.clone(), clock.clone());
        match self {
            EngineTime::Real => epoch::tick_in_real_time(ticks),
            EngineTime::Manual(time) => {
                ensure!(time.drive(ticks), "this ManualTime moves another engine");
            }
        }
        Ok(clock)
    }
}

/// Time a test moves, for one engine: the handle the test keeps, as Waft's tests keep their
/// `AppCell`. Its clones move the same time.
#[derive(Clone)]
pub struct ManualTime(Arc<Manual>);

struct Manual {
    /// The moment the engine's clock reads.
    now: Arc<Mutex<Instant>>,
    /// How far time has moved since the last tick ran, held while a move runs, so moves from two
    /// threads run their ticks one after the other.
    since_tick: Mutex<Duration>,
    /// What each tick moves, once an engine is made with this time.
    ticks: OnceLock<Ticks>,
}

/// The engine's clock, which every clone reads, and the timers its ticks run.
#[derive(Clone)]
pub(crate) struct EngineClock(Arc<ClockParts>);

struct ClockParts {
    source: Source,
    /// What runs at every tick, held weakly: a timer dropped by its owner runs no more.
    timers: Mutex<Vec<Weak<Timer>>>,
}

/// What runs at a tick, with the clock's time then.
pub(crate) type Timer = dyn Fn(Instant) + Send + Sync;

enum Source {
    Real,
    Manual(Arc<Mutex<Instant>>),
}

impl ManualTime {
    /// Time standing at the moment it is made, until a test moves it.
    pub fn new() -> ManualTime {
        ManualTime(Arc::new(Manual {
            now: Arc::new(Mutex::new(Instant::now())),
            since_tick: Mutex::new(Duration::ZERO),
            ticks: OnceLock::new(),
        }))
    }

    /// Moves time by `by`, running each 100 ms tick on the way, in order: the timers, the freezes
    /// that come due, then the epoch.
    pub fn elapse(&self, by: Duration) {
        let mut since_tick = locked(&self.0.since_tick);
        let mut left = by;
        while *since_tick + left >= TICK {
            let to_tick = TICK - *since_tick;
            self.advance(to_tick);
            left -= to_tick;
            *since_tick = Duration::ZERO;
            if let Some(ticks) = self.0.ticks.get() {
                ticks.run();
            }
        }
        self.advance(left);
        *since_tick += left;
    }

    /// The clock an engine made with this time reads.
    fn clock(&self) -> EngineClock {
        EngineClock::with_source(Source::Manual(Arc::clone(&self.0.now)))
    }

    /// Lets this time move the engine `ticks` belong to; answers false when it moves another
    /// already.
    fn drive(&self, ticks: Ticks) -> bool {
        self.0.ticks.set(ticks).is_ok()
    }

    fn advance(&self, by: Duration) {
        *locked(&self.0.now) += by;
    }
}

impl Default for ManualTime {
    fn default() -> ManualTime {
        ManualTime::new()
    }
}

impl fmt::Debug for ManualTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManualTime")
            .field("now", &*locked(&self.0.now))
            .finish()
    }
}

impl EngineClock {
    /// The system's clock.
    pub(crate) fn real() -> EngineClock {
        EngineClock::with_source(Source::Real)
    }

    fn with_source(source: Source) -> EngineClock {
        EngineClock(Arc::new(ClockParts {
            source,
            timers: Mutex::default(),
        }))
    }

    pub(crate) fn now(&self) -> Instant {
        match &self.0.source {
            Source::Real => Instant::now(),
            Source::Manual(now) => *locked(now),
        }
    }

    /// Runs `timer` at every tick from now on, for as long as its owner holds it.
    pub(crate) fn at_each_tick(&self, timer: &Arc<Timer>) {
        locked(&self.0.timers).push(Arc::downgrade(timer));
    }

    /// Runs every timer held, with the clock at `now`, the dropped ones forgotten.
    pub(crate) fn run_timers(&self, now: Instant) {
        let timers: Vec<Arc<Timer>> = {
            let mut timers = locked(&self.0.timers);
            timers.retain(|timer| timer.strong_count() > 0);
            timers.iter().filter_map(Weak::upgrade).collect()
        };
        for timer in timers {
            timer(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::engine::engine_config;

    #[test]
    fn manual_time_stands_until_moved() {
        let time = ManualTime::new();
        let clock = time.clock();
        let before = clock.now();
        assert_eq!(clock.now(), before);
        time.elapse(Duration::from_millis(250));
        assert_eq!(clock.now() - before, Duration::from_millis(250));
    }

    #[test]
    fn moving_time_runs_each_tick_it_passes_at_its_own_moment() {
        let time = ManualTime::new();
        let engine = Arc::new(Engine::new(&engine_config(false)).unwrap());
        let clock = EngineTime::Manual(time.clone())
            .start(&engine, &Programs::default())
            .unwrap();
        let start = clock.now();
        let ticks = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&ticks);
        let timer: Arc<Timer> = Arc::new(move |now| kept.lock().unwrap().push(now - start));
        clock.at_each_tick(&timer);
        time.elapse(Duration::from_millis(250));
        time.elapse(Duration::from_millis(50));
        let millis = |millis| Duration::from_millis(millis);
        assert_eq!(
            *ticks.lock().unwrap(),
            [millis(100), millis(200), millis(300)]
        );
        assert!(
            EngineTime::Manual(time)
                .start(&engine, &Programs::default())
                .is_err(),
            "one engine for one time"
        );
    }

    #[test]
    fn a_timer_runs_while_held_and_not_once_dropped() {
        let clock = EngineClock::real();
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let timer: Arc<Timer> = Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        clock.at_each_tick(&timer);
        clock.run_timers(clock.now());
        drop(timer);
        clock.run_timers(clock.now());
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }
}
