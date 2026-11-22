//! The screens' refreshes the engine waits for, shared by every program's views: a view that
//! wants a frame asks for the next refresh of the screen it is on, and the shell hears one request
//! per screen until that refresh comes, however many views on it ask. A refresh, as the shell
//! reports it, is a [`Refresh`], and the last one each screen had is kept, which is how the
//! engine knows the refresh a screen is in when a view asks for a frame between two.
//!
//! The refresh a screen is in is worked out as Chromium's `DelayBasedBeginFrameSource` works out
//! the tick an observer added between two missed: the last refresh the shell told of, moved on by
//! as many whole intervals of the screen as have passed since, the interval being the refresh
//! rate the system says the screen has. Where the system says no rate, or no refresh of the
//! screen was told yet, the refresh it is in is not known, and nothing here guesses it.
//!
//! The invariants: a screen's refresh is asked for at most once until it comes, and its request is
//! forgotten before the refresh serves anyone, so a view that asks while it is served asks the
//! shell again, for the refresh after; and where the screen's interval is known, the refresh after
//! each one a view was served a frame of is asked for, so the last refresh told is never one a
//! view has had while a later one has begun, and how late the shell stamps a refresh never
//! decides whether a view has had the one its screen is in.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::bindings::wasdk::platform::screens::ScreenId;
use crate::running::Programs;
use crate::screens::Screens;
use crate::shell::VsyncWaiter;
use crate::sync::locked;

/// One refresh of a screen, as the shell reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refresh {
    /// When it began.
    pub(crate) at: Instant,
    /// When the picture drawn for it is expected on screen.
    pub(crate) display_time: Instant,
}

impl Refresh {
    /// The refresh a screen is in at `now`, when this one is the last it was told to have and
    /// its refreshes are `interval` apart: this one moved on by the whole intervals passed since
    /// it began, its picture expected as long after it begins as this one's.
    fn moved_on_to(self, now: Instant, interval: Duration) -> Refresh {
        let since = now.saturating_duration_since(self.at);
        let intervals = since.as_nanos() / interval.as_nanos().max(1);
        let moved_on = Duration::from_nanos((intervals * interval.as_nanos()) as u64);
        Refresh {
            at: self.at + moved_on,
            display_time: self.display_time + moved_on,
        }
    }
}

/// The screens' refreshes: those asked of the shell and not come yet, and the last each screen
/// had. Every clone reaches the same ones.
#[derive(Clone)]
pub(crate) struct ScreenRefreshes(Arc<Refreshes>);

struct Refreshes {
    waiter: Arc<dyn VsyncWaiter>,
    /// The screens as the system has them, whose refresh rates give their intervals.
    screens: Screens,
    /// The programs running, whose views a refresh serves.
    programs: Programs,
    of_screens: Mutex<HashMap<ScreenId, ScreenRefresh>>,
}

/// What is known of one screen's refreshes.
#[derive(Default)]
struct ScreenRefresh {
    /// Whether the next was asked of the shell and has not come.
    next_asked: bool,
    /// The last the shell told of.
    last: Option<Refresh>,
}

impl ScreenRefreshes {
    /// None asked yet, of the shell's `waiter`, for the screens `screens`, a refresh serving the
    /// views of `programs`.
    pub(crate) fn new(
        waiter: Arc<dyn VsyncWaiter>,
        screens: Screens,
        programs: Programs,
    ) -> ScreenRefreshes {
        ScreenRefreshes(Arc::new(Refreshes {
            waiter,
            screens,
            programs,
            of_screens: Mutex::default(),
        }))
    }

    /// Asks the shell for the next refresh of `screen`, unless it was asked and has not come.
    pub(crate) fn ask(&self, screen: ScreenId) {
        let newly_asked = {
            let mut of_screens = locked(&self.0.of_screens);
            let of_screen = of_screens.entry(screen).or_default();
            !std::mem::replace(&mut of_screen.next_asked, true)
        };
        if newly_asked {
            // Weakly: a refresh the shell holds on to keeps nothing of an engine gone.
            let refreshes = Arc::downgrade(&self.0);
            self.0.waiter.next_vsync(
                screen,
                Box::new(move |at, display_time| {
                    refreshed(&refreshes, screen, Refresh { at, display_time });
                }),
            );
        }
    }

    /// `refresh` of `screen` came: it is the last the screen had, and the next one is asked for
    /// anew.
    pub(crate) fn came(&self, screen: ScreenId, refresh: Refresh) {
        let mut of_screens = locked(&self.0.of_screens);
        let of_screen = of_screens.entry(screen).or_default();
        of_screen.next_asked = false;
        of_screen.last = Some(refresh);
    }

    /// The refresh `screen` is in at `now`; none where the system does not say how often the
    /// screen refreshes, or the shell told of no refresh of it yet.
    pub(crate) fn current(&self, screen: ScreenId, now: Instant) -> Option<Refresh> {
        let interval = self.interval_of(screen)?;
        let last = locked(&self.0.of_screens).get(&screen)?.last?;
        Some(last.moved_on_to(now, interval))
    }

    /// A view on `screen` was served a frame: the refresh after is asked for where the screen's
    /// interval is known, so the engine hears of it though no view waits for it.
    pub(crate) fn frame_served(&self, screen: ScreenId) {
        if self.interval_of(screen).is_some() {
            self.ask(screen);
        }
    }

    /// How far apart `screen`'s refreshes are, by the rate the system says it has; none where it
    /// says none, or has no such screen.
    fn interval_of(&self, screen: ScreenId) -> Option<Duration> {
        const NANOSECONDS_PER_KILOSECOND: u64 = 1_000_000_000_000;
        let millihertz = u64::from(self.0.screens.find(screen)?.refresh_rate);
        (millihertz > 0).then(|| Duration::from_nanos(NANOSECONDS_PER_KILOSECOND / millihertz))
    }
}

/// `refresh` of `screen` came, as the shell reports what it was asked: it is the last the screen
/// had, and each program's views on it that asked for a frame and are in sight get one, the frame
/// naming them all.
fn refreshed(refreshes: &Weak<Refreshes>, screen: ScreenId, refresh: Refresh) {
    let Some(refreshes) = refreshes.upgrade() else {
        return;
    };
    let programs = refreshes.programs.clone();
    ScreenRefreshes(refreshes).came(screen, refresh);
    programs.screen_refreshed(screen, refresh);
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::LazyLock;

    use crate::screens::tests::screen_refreshing_at;
    use crate::shell::tests::Headless;

    use super::*;

    /// The interval of a screen at 1 Hz, slow enough that a test is over within one refresh.
    pub(crate) const A_SECOND: Duration = Duration::from_secs(1);

    /// The refreshes asked of `shell`, for no screen the system says a rate of.
    pub(crate) fn refreshes_of(shell: &Arc<Headless>) -> ScreenRefreshes {
        ScreenRefreshes::new(
            Arc::clone(shell) as Arc<dyn VsyncWaiter>,
            Screens::new(Vec::new()),
            Programs::default(),
        )
    }

    /// The refreshes asked of `shell`, for the screen 1, which refreshes once a second.
    pub(crate) fn refreshes_a_second_apart(shell: &Arc<Headless>) -> ScreenRefreshes {
        ScreenRefreshes::new(
            Arc::clone(shell) as Arc<dyn VsyncWaiter>,
            Screens::new(vec![screen_refreshing_at(1, 1_000)]),
            Programs::default(),
        )
    }

    /// A refresh that began at `at`, its picture expected 3 ms later.
    pub(crate) fn refresh_at(at: Instant) -> Refresh {
        Refresh {
            at,
            display_time: at + Duration::from_millis(3),
        }
    }

    /// The `number`th refresh of a screen that refreshes once a second.
    pub(crate) fn refresh(number: u32) -> Refresh {
        static FIRST: LazyLock<Instant> = LazyLock::new(Instant::now);
        refresh_at(*FIRST + A_SECOND * number)
    }

    #[test]
    fn a_screens_refresh_is_asked_for_once_until_it_comes() {
        let shell = Arc::new(Headless::default());
        let refreshes = refreshes_of(&shell);
        refreshes.ask(1);
        refreshes.ask(1);
        refreshes.ask(2);
        assert_eq!(shell.refreshes_asked(), [1, 2], "once per screen");
        refreshes.came(1, refresh(0));
        refreshes.ask(1);
        refreshes.ask(2);
        assert_eq!(
            shell.refreshes_asked(),
            [1, 2, 1],
            "come, the next is asked for anew; the other still waits"
        );
    }

    #[test]
    fn a_screen_is_in_the_last_refresh_told_until_its_interval_has_passed() {
        let refreshes = refreshes_a_second_apart(&Arc::default());
        let told = refresh(0);
        refreshes.came(1, told);
        assert_eq!(refreshes.current(1, told.at), Some(told));
        let almost_the_next = told.at + A_SECOND - Duration::from_nanos(1);
        assert_eq!(refreshes.current(1, almost_the_next), Some(told));
    }

    #[test]
    fn a_screen_not_told_of_since_is_in_the_refresh_whole_intervals_after_the_last_told() {
        let refreshes = refreshes_a_second_apart(&Arc::default());
        let told = refresh(0);
        refreshes.came(1, told);
        assert_eq!(refreshes.current(1, told.at + A_SECOND), Some(refresh(1)));
        let long_after = told.at + A_SECOND * 3_600 + A_SECOND / 2;
        assert_eq!(
            refreshes.current(1, long_after),
            Some(refresh(3_600)),
            "its picture expected as long after it begins as the last told"
        );
    }

    #[test]
    fn a_refresh_stamped_ahead_of_now_is_the_one_the_screen_is_in() {
        let refreshes = refreshes_a_second_apart(&Arc::default());
        let told = refresh(1);
        refreshes.came(1, told);
        assert_eq!(refreshes.current(1, refresh(0).at), Some(told));
    }

    #[test]
    fn the_refresh_a_screen_is_in_is_not_known_before_one_was_told_or_without_its_rate() {
        let shell = Arc::new(Headless::default());
        let refreshes = refreshes_a_second_apart(&shell);
        let told = refresh(0);
        assert_eq!(refreshes.current(1, told.at), None, "none told yet");
        refreshes.came(2, told);
        assert_eq!(
            refreshes.current(2, told.at),
            None,
            "the system has no screen 2 to say a rate of"
        );
        let unsaid = ScreenRefreshes::new(
            Arc::clone(&shell) as Arc<dyn VsyncWaiter>,
            Screens::new(vec![screen_refreshing_at(1, 0)]),
            Programs::default(),
        );
        unsaid.came(1, told);
        assert_eq!(unsaid.current(1, told.at), None, "the system says no rate");
    }

    #[test]
    fn the_refresh_after_a_frame_served_is_asked_for_only_where_the_interval_is_known() {
        let shell = Arc::new(Headless::default());
        refreshes_of(&shell).frame_served(1);
        assert!(shell.refreshes_asked().is_empty(), "no rate: nothing more");
        let refreshes = refreshes_a_second_apart(&shell);
        refreshes.frame_served(1);
        refreshes.frame_served(1);
        assert_eq!(shell.refreshes_asked(), [1], "once until it comes");
    }
}
