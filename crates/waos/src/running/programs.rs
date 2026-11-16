//! Every program the engine runs now, for what the engine does to all of them at once:
//! suspending and resuming the app, reading the clock for each one's freeze rule, and serving a
//! screen's refresh.
//!
//! The invariant: a program joins before its code runs, under the lock that holds whether the
//! app is suspended, so one started while the app is suspended starts paused and frozen; one
//! that ended is forgotten once every handle on it is dropped, and until then is told what every
//! program is told, which does nothing to a program with no store.

use std::future::Future;
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use crate::bindings::wasdk::platform::screens::ScreenId;
use crate::sync::locked;
use crate::views::Refresh;

use super::{Running, RunningProgram};

/// The programs running now. Every clone reaches the same list.
#[derive(Clone, Default)]
pub(crate) struct Programs(Arc<Mutex<Roster>>);

#[derive(Default)]
struct Roster {
    programs: Vec<Weak<Running>>,
    app_suspended: bool,
}

impl Programs {
    /// `program` joins: told what every program is told from now on, and paused and frozen at
    /// once while the app is suspended.
    pub(crate) fn add(&self, program: &RunningProgram) {
        let mut roster = locked(&self.0);
        roster.programs.push(Arc::downgrade(&program.0));
        if roster.app_suspended {
            program.lifecycle().app_suspending();
        }
    }

    /// The program running under `name`, the one that started last where an earlier one is still
    /// ending; none once it ended.
    pub(crate) fn named(&self, name: &str) -> Option<RunningProgram> {
        let programs = locked(&self.0).live();
        programs
            .into_iter()
            .rev()
            .find(|program| program.name() == name && !program.has_ended())
    }

    /// The app is suspending: every program is paused and frozen at once. Answers, for each,
    /// when it has stopped.
    pub(crate) fn suspend(&self) -> Vec<impl Future<Output = ()> + Send + 'static + use<>> {
        let mut roster = locked(&self.0);
        roster.app_suspended = true;
        roster
            .live()
            .iter()
            .map(|program| {
                program.lifecycle().app_suspending();
                program.idle()
            })
            .collect()
    }

    /// The app resumed: every program is back in the state its views give, and thawed if that
    /// shows it.
    pub(crate) fn resume(&self) {
        let mut roster = locked(&self.0);
        roster.app_suspended = false;
        for program in roster.live() {
            program.lifecycle().app_resumed();
        }
    }

    /// `screen` refreshed, as `refresh` says: each program's views on it that wait for a frame
    /// and are in sight get one, a frame per program naming its views it serves.
    pub(crate) fn screen_refreshed(&self, screen: ScreenId, refresh: Refresh) {
        let programs = locked(&self.0).live();
        for program in programs {
            program.program_views().screen_refreshed(screen, refresh);
        }
    }

    /// The clock reads `now`: every program hidden for its delay is frozen.
    pub(crate) fn freeze_if_due(&self, now: Instant) {
        for program in locked(&self.0).live() {
            program.lifecycle().freeze_if_due(now);
        }
    }
}

impl Roster {
    /// The programs still held, the gone ones forgotten.
    fn live(&mut self) -> Vec<RunningProgram> {
        self.programs.retain(|program| program.strong_count() > 0);
        self.programs
            .iter()
            .filter_map(Weak::upgrade)
            .map(RunningProgram)
            .collect()
    }
}
