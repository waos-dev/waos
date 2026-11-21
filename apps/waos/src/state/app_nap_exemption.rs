//! macOS's App Nap kept from throttling WaOS while the user waits on a program: one activity
//! held from the moment any program's code begins compiling or one of its windows comes into
//! sight until none does, as Chrome holds one in a renderer while its tab is in front. App Nap
//! otherwise runs a process it thinks is in the background at low priority, which made a compile
//! that takes seconds take minutes (measured: a 5-second compile took 9 minutes 15 seconds without
//! the activity, and 5 seconds with it).
//!
//! The activity is the system's, behind the closure this state is made with, so its rule is tested
//! without macOS. [`start`] makes it over Waft's macOS host; [`follow_installed_compiling`],
//! [`follow_dev_compiling`] and [`follow_windows`] tell it what the library's compiles, the
//! compiles `waos`'s runs make and the programs' windows report. None of this is built on
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
/// whose code compiles and the programs' windows in sight. One activity is held while either is
/// non-empty, begun when the first comes and ended when the last goes.
pub(crate) struct AppNapExemption {
    begin: Begin,
    /// macOS's activity, held while [`needed`](Self::needed) is true; none before the first
    /// reason and after the last.
    held: Option<Box<dyn Any>>,
    /// The programs whose code the library compiles, being compiled now.
    installed_compiling: BTreeSet<ProgramId>,
    /// The programs whose code `waos` sent and WaOS is compiling now.
    sent_compiling: BTreeSet<ProgramId>,
    /// The programs' windows in sight now.
    in_sight: BTreeSet<WindowKey>,
}

impl AppNapExemption {
    /// The activity begun by `begin`, which tells macOS WaOS is doing work its user waits on.
    pub(crate) fn new(begin: Begin) -> AppNapExemption {
        AppNapExemption {
            begin,
            held: None,
            installed_compiling: BTreeSet::new(),
            sent_compiling: BTreeSet::new(),
            in_sight: BTreeSet::new(),
        }
    }

    /// The programs whose code is being compiled are `compiling`.
    pub(crate) fn installed_compiling_changed(&mut self, compiling: BTreeSet<ProgramId>) {
        self.installed_compiling = compiling;
        self.keep_while_needed();
    }

    /// The programs whose code `waos` sent and WaOS is compiling are `compiling`.
    pub(crate) fn sent_compiling_changed(&mut self, compiling: BTreeSet<ProgramId>) {
        self.sent_compiling = compiling;
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
        !self.installed_compiling.is_empty()
            || !self.sent_compiling.is_empty()
            || !self.in_sight.is_empty()
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
    use std::collections::BTreeSet;

    use futures::StreamExt;
    use futures::channel::mpsc::UnboundedReceiver;
    use tokio::sync::watch;
    use waft::{App, Entity};
    use waos_package::ProgramId;

    use super::{AppNapExemption, WindowKey};
    use crate::logic::program_apps::WindowPresence;
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

    /// Keeps the activity while the programs whose code is being compiled are those `compiling`
    /// tells, from now on.
    pub(crate) fn follow_installed_compiling(
        app: &mut App,
        activity: Entity<AppNapExemption>,
        mut compiling: watch::Receiver<BTreeSet<ProgramId>>,
    ) {
        let now = compiling.borrow_and_update().clone();
        activity.update(app, |activity, _| activity.installed_compiling_changed(now));
        app.spawn(async move |cx| {
            while compiling.changed().await.is_ok() {
                let now = compiling.borrow_and_update().clone();
                cx.update(|app| {
                    activity.update(app, |activity, _| activity.installed_compiling_changed(now));
                });
            }
        });
    }

    /// Keeps the activity while the programs `waos` sent are being compiled.
    pub(crate) fn follow_dev_compiling(
        app: &mut App,
        activity: Entity<AppNapExemption>,
        mut compiling: watch::Receiver<BTreeSet<ProgramId>>,
    ) {
        let now = compiling.borrow_and_update().clone();
        activity.update(app, |activity, _| activity.sent_compiling_changed(now));
        app.spawn(async move |cx| {
            while compiling.changed().await.is_ok() {
                let now = compiling.borrow_and_update().clone();
                cx.update(|app| {
                    activity.update(app, |activity, _| activity.sent_compiling_changed(now));
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
pub(crate) use host::{follow_dev_compiling, follow_installed_compiling, follow_windows, start};

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

        activity.installed_compiling_changed([first.clone()].into());
        assert!(activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 0));

        activity.window_in_sight_changed((first.clone(), 1), true);
        activity.installed_compiling_changed([first.clone(), second.clone()].into());
        assert_eq!(
            (begun.get(), ended.get()),
            (1, 0),
            "one activity for every reason together"
        );

        activity.installed_compiling_changed(BTreeSet::new());
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
    fn a_compile_cargo_waos_sent_holds_it_too() {
        let (mut activity, begun, ended) = activity();
        let sent: ProgramId = "dev.example.sent".parse().unwrap();
        activity.sent_compiling_changed([sent.clone()].into());
        assert!(activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 0));

        activity.sent_compiling_changed(BTreeSet::new());
        assert!(!activity.needed());
        assert_eq!((begun.get(), ended.get()), (1, 1));
    }
}
