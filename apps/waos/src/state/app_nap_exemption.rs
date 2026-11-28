//! macOS's App Nap kept from throttling WaOS while the user waits on a program: one activity
//! held from the moment the package manager begins working on any program, as it compiles its
//! code, or a start begins optimizing one, compiling its code again, or one of a program's windows
//! comes into
//! sight until none does, as Chrome holds one in a renderer while its tab is in front. App Nap
//! otherwise runs a process it thinks is in the background at low priority, which made a compile
//! that takes seconds take minutes (measured: a 5-second compile took 9 minutes 15 seconds without
//! the activity, and 5 seconds with it).
//!
//! The activity is the system's, behind the closure this state is made with, so its rule is tested
//! without macOS. [`start`] makes it over Waft's macOS host; [`follow_operations`],
//! [`follow_starts`] and [`follow_windows`] tell it what the package manager is doing to which
//! program, a development install among them, which starts optimize, and what the programs'
//! windows report. None of this is built on
//! another system, where there is nothing to keep.

use std::any::Any;
use std::collections::BTreeSet;

use waos::bindings::wasdk::platform::views::RequestId;
use waos_package::ProgramId;

/// One of a program's windows: the program it shows and the request WaOS made it for, which two
/// programs' windows may share.
pub(crate) type WindowKey = (ProgramId, RequestId);

/// What begins the work macOS must not throttle and the value that ends it when dropped: macOS's
/// `UserActivity` in WaOS, a test's own. None where the platform has no such call.
type Begin = Box<dyn Fn() -> Option<Box<dyn Any>>>;

/// Why WaOS must keep the priority macOS gives an app its user is waiting on now: the programs
/// the package manager works on, the starts optimizing and the programs' windows in sight. One
/// activity is held while any is non-empty, begun when the first comes and ended when the last
/// goes.
pub(crate) struct AppNapExemption {
    begin: Begin,
    /// macOS's activity, held while [`needed`](Self::needed) is true; none before the first
    /// reason and after the last.
    held: Option<Box<dyn Any>>,
    /// The programs the package manager works on now: downloading, installing (compiling) or
    /// removing.
    operated: BTreeSet<ProgramId>,
    /// The apps whose start compiles their code again now.
    optimizing: BTreeSet<ProgramId>,
    /// The programs' windows in sight now.
    in_sight: BTreeSet<WindowKey>,
}

impl AppNapExemption {
    /// The activity begun by `begin`, which tells macOS WaOS is doing work its user waits on.
    pub(crate) fn new(begin: Begin) -> AppNapExemption {
        AppNapExemption {
            begin,
            held: None,
            operated: BTreeSet::new(),
            optimizing: BTreeSet::new(),
            in_sight: BTreeSet::new(),
        }
    }

    /// The programs the package manager works on are `operated`.
    pub(crate) fn operated_changed(&mut self, operated: BTreeSet<ProgramId>) {
        self.operated = operated;
        self.keep_while_needed();
    }

    /// The apps whose start compiles their code again are `optimizing`.
    pub(crate) fn optimizing_changed(&mut self, optimizing: BTreeSet<ProgramId>) {
        self.optimizing = optimizing;
        self.keep_while_needed();
    }

    /// The window `window` of the program `program` is in sight, or no longer.
    pub(crate) fn window_in_sight_changed(&mut self, window: WindowKey, in_sight: bool) {
        if in_sight {
            self.in_sight.insert(window);
        } else {
            self.in_sight.remove(&window);
        }
        self.keep_while_needed();
    }

    /// Whether WaOS is doing work its user waits on now.
    pub(crate) fn needed(&self) -> bool {
        !self.operated.is_empty() || !self.optimizing.is_empty() || !self.in_sight.is_empty()
    }

    /// Begins the activity with the first reason and ends it with the last.
    fn keep_while_needed(&mut self) {
        if self.needed() {
            if self.held.is_none() {
                self.held = (self.begin)();
                if self.held.is_some() {
                    tracing::info!(
                        "macOS keeps WaOS from throttling it: a program compiles or its window is in sight"
                    );
                }
            }
        } else if self.held.take().is_some() {
            tracing::info!(
                "macOS may throttle WaOS again: no program compiles and none shows a window"
            );
        }
    }
}

#[cfg(target_os = "macos")]
mod host {
    use std::any::Any;
    use std::collections::{BTreeMap, BTreeSet};

    use futures::StreamExt;
    use futures::channel::mpsc::UnboundedReceiver;

    use tokio::sync::watch;
    use waft::{App, Entity};
    use waos_package::ProgramId;
    use waos_services::packages::InstallSessionInfo;

    use super::{AppNapExemption, WindowKey};
    use crate::logic::app_instances::OpenStage;
    use crate::logic::macos_shell::WindowPresence;
    use crate::state::desktop_host::with_macos_platform;

    /// Kept for the system's logs: what the user is waiting on.
    const REASON: &str = "WaOS is compiling or showing a program";

    /// The activity of WaOS over Waft's macOS host, begun and ended as the reasons come and go.
    pub(crate) fn start(app: &mut App) -> Entity<AppNapExemption> {
        let platform = app.platform();
        let begin = Box::new(move || {
            with_macos_platform(&platform, |macos| {
                Box::new(macos.begin_user_activity(REASON)) as Box<dyn Any>
            })
        });
        app.new_entity(|_| AppNapExemption::new(begin))
    }

    /// Keeps the activity while the package manager installs any app, as `sessions` tells, from
    /// now on.
    pub(crate) fn follow_sessions(
        app: &mut App,
        activity: Entity<AppNapExemption>,
        mut sessions: watch::Receiver<Vec<InstallSessionInfo>>,
    ) {
        let apps_of = |sessions: &[InstallSessionInfo]| -> BTreeSet<ProgramId> {
            sessions.iter().map(|session| session.app.clone()).collect()
        };
        let now = apps_of(&sessions.borrow_and_update());
        activity.update(app, |activity, _| activity.operated_changed(now));
        app.spawn(async move |cx| {
            while sessions.changed().await.is_ok() {
                let now = apps_of(&sessions.borrow_and_update());
                cx.update(|app| {
                    activity.update(app, |activity, _| activity.operated_changed(now));
                });
            }
        });
    }

    /// Keeps the activity while a start optimizes any app, as `starting` tells, from now on.
    pub(crate) fn follow_starts(
        app: &mut App,
        activity: Entity<AppNapExemption>,
        mut starting: watch::Receiver<BTreeMap<ProgramId, OpenStage>>,
    ) {
        let optimizing = |starting: &BTreeMap<ProgramId, OpenStage>| {
            starting
                .iter()
                .filter(|(_, stage)| **stage == OpenStage::Optimizing)
                .map(|(id, _)| id.clone())
                .collect()
        };
        let now = optimizing(&starting.borrow_and_update());
        activity.update(app, |activity, _| activity.optimizing_changed(now));
        app.spawn(async move |cx| {
            while starting.changed().await.is_ok() {
                let now = optimizing(&starting.borrow_and_update());
                cx.update(|app| {
                    activity.update(app, |activity, _| activity.optimizing_changed(now));
                });
            }
        });
    }

    /// Keeps the activity while a window the programs' apps report in sight is.
    pub(crate) fn follow_windows(
        app: &mut App,
        activity: Entity<AppNapExemption>,
        mut reported: UnboundedReceiver<WindowPresence>,
    ) {
        app.spawn(async move |cx| {
            while let Some(presence) = reported.next().await {
                let window: WindowKey = (presence.program, presence.window);
                cx.update(|app| {
                    activity.update(app, |activity, _| {
                        activity.window_in_sight_changed(window, presence.in_sight)
                    });
                });
            }
        });
    }
}

#[cfg(target_os = "macos")]
pub(crate) use host::{follow_sessions, follow_starts, follow_windows, start};

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    /// An activity of the test's own, which counts how many times one is begun and ended.
    struct Held {
        ended: Rc<Cell<usize>>,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            self.ended.set(self.ended.get() + 1);
        }
    }

    /// The activity rule over a test's activity, and the counts of what it began and ended.
    fn activity() -> (AppNapExemption, Rc<Cell<usize>>, Rc<Cell<usize>>) {
        let begun = Rc::new(Cell::new(0));
        let ended = Rc::new(Cell::new(0));
        let (b, e) = (Rc::clone(&begun), Rc::clone(&ended));
        let begin = Box::new(move || {
            b.set(b.get() + 1);
            Some(Box::new(Held {
                ended: Rc::clone(&e),
            }) as Box<dyn Any>)
        });
        (AppNapExemption::new(begin), begun, ended)
    }

    #[test]
    fn one_activity_while_a_program_compiles_or_a_window_is_in_sight() {
        let (mut activity, begun, ended) = activity();
        let first: ProgramId = "dev.example.first".parse().unwrap();
        let second: ProgramId = "dev.example.second".parse().unwrap();
        assert!(!activity.needed());
        assert_eq!((begun.get(), ended.get()), (0, 0));

        activity.operated_changed([first.clone()].into());
        assert!(activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 0));

        activity.window_in_sight_changed((first.clone(), 1), true);
        activity.operated_changed([first.clone(), second.clone()].into());
        assert_eq!(
            (begun.get(), ended.get()),
            (1, 0),
            "one activity for every reason together"
        );

        activity.operated_changed(BTreeSet::new());
        assert_eq!((begun.get(), ended.get()), (1, 0), "a window keeps it");

        activity.window_in_sight_changed((first.clone(), 1), true);
        assert_eq!(
            (begun.get(), ended.get()),
            (1, 0),
            "the same window again is no new reason"
        );

        activity.window_in_sight_changed((first, 1), false);
        assert!(!activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 1));

        activity.window_in_sight_changed((second.clone(), 1), true);
        assert_eq!(
            (begun.get(), ended.get()),
            (2, 1),
            "another window begins it again"
        );

        activity.window_in_sight_changed((second, 1), false);
        assert_eq!((begun.get(), ended.get()), (2, 2));
    }

    #[test]
    fn a_development_installs_compile_holds_it_as_any_installs_does() {
        let (mut activity, begun, ended) = activity();
        let development: ProgramId = "dev.example.notes.development".parse().unwrap();
        activity.operated_changed([development].into());
        assert!(activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 0));

        activity.operated_changed(BTreeSet::new());
        assert!(!activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 1));
    }

    #[test]
    fn a_start_optimizing_an_app_holds_it() {
        let (mut activity, begun, ended) = activity();
        let notes: ProgramId = "dev.example.notes".parse().unwrap();
        activity.optimizing_changed([notes].into());
        assert!(activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 0));

        activity.optimizing_changed(BTreeSet::new());
        assert!(!activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 1));
    }
}
