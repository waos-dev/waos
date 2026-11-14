//! The screens' refreshes the engine asks for, over Waft's vsync waiter: the next refresh of a
//! screen is a wait on Waft's, which serves it at the vsync Waft's own frames on that screen are
//! drawn at, and its call reports the refresh to the engine, with when the picture drawn for it
//! reaches the screen, which serves the programs' views on that screen. A screen is the same on both sides: the engine's screen id is Waft's.
//!
//! The invariant: the engine is reached weakly, since it holds this as its shell's; and a screen
//! has one observer for as long as the app runs, so the engine's ask, at most one per screen until
//! its refresh comes, is one wait on Waft's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use waft::ui::{Instant, ScreenId, VsyncObserver, VsyncWaiter as WaftVsyncWaiter};
use waos::{VsyncWaiter, WaOS, targets};

/// The engine's refreshes, as Waft's waiter serves them.
pub(crate) struct WaftVsync {
    waiter: Arc<dyn WaftVsyncWaiter>,
    /// The engine the refreshes are reported to, once it is made.
    engine: Arc<OnceLock<Weak<WaOS>>>,
    /// What reports each screen's refresh, made at its first.
    observers: Mutex<HashMap<u64, Arc<ScreenRefresh>>>,
}

/// Reports a refresh of one screen to the engine.
struct ScreenRefresh {
    screen: u64,
    engine: Arc<OnceLock<Weak<WaOS>>>,
}

impl WaftVsync {
    /// Refreshes served by `waiter`, Waft's, reported to the engine once it is attached.
    pub(crate) fn new(waiter: Arc<dyn WaftVsyncWaiter>) -> WaftVsync {
        WaftVsync {
            waiter,
            engine: Arc::new(OnceLock::new()),
            observers: Mutex::default(),
        }
    }

    /// The refreshes go to `engine` from now on.
    pub(crate) fn attach(&self, engine: &Arc<WaOS>) {
        let _ = self.engine.set(Arc::downgrade(engine));
    }

    /// What reports `screen`'s refreshes.
    fn observer_of(&self, screen: u64) -> Arc<ScreenRefresh> {
        // Nothing panics under the lock, so it is never poisoned; one that was is still sound.
        let mut observers = self
            .observers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let observer = observers.entry(screen).or_insert_with(|| {
            Arc::new(ScreenRefresh {
                screen,
                engine: Arc::clone(&self.engine),
            })
        });
        Arc::clone(observer)
    }
}

impl VsyncWaiter for WaftVsync {
    fn next_vsync(&self, screen: u64) {
        let observer = self.observer_of(screen);
        tracing::trace!(target: targets::FRAMES, screen, "the shell waits for the screen's next vsync");
        let observer = Arc::downgrade(&observer) as Weak<dyn VsyncObserver>;
        self.waiter.next_vsync(ScreenId(screen), observer);
    }
}

impl VsyncObserver for ScreenRefresh {
    fn vsync(&self, at: Instant, display_time: Instant) {
        tracing::trace!(target: targets::FRAMES, screen = self.screen, "the screen's vsync");
        if let Some(engine) = self.engine.get().and_then(Weak::upgrade) {
            engine.screen_refreshed(self.screen, at, display_time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A waiter that keeps every wait.
    #[derive(Default)]
    struct Waits(Mutex<Vec<(ScreenId, Weak<dyn VsyncObserver>)>>);

    impl WaftVsyncWaiter for Waits {
        fn next_vsync(&self, screen: ScreenId, observer: Weak<dyn VsyncObserver>) {
            self.0.lock().unwrap().push((screen, observer));
        }
    }

    #[test]
    fn a_screens_refresh_is_a_wait_on_wafts_by_the_same_observer_each_time() {
        let waits = Arc::new(Waits::default());
        let vsync = WaftVsync::new(Arc::clone(&waits) as Arc<dyn WaftVsyncWaiter>);
        vsync.next_vsync(3);
        vsync.next_vsync(3);
        vsync.next_vsync(5);
        let waits = waits.0.lock().unwrap();
        let screens: Vec<ScreenId> = waits.iter().map(|(screen, _)| *screen).collect();
        assert_eq!(screens, [ScreenId(3), ScreenId(3), ScreenId(5)]);
        assert!(
            Weak::ptr_eq(&waits[0].1, &waits[1].1),
            "one observer per screen, so Waft calls it once a refresh"
        );
        assert!(waits[0].1.upgrade().is_some(), "kept while the app runs");
    }

    /// The engine asks from the programs' threads and Waft calls back on the main thread.
    #[test]
    fn the_refreshes_cross_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<WaftVsync>();
        assert_shared::<ScreenRefresh>();
    }
}
