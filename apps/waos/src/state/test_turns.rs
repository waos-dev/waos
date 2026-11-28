//! Turning a test's app until what the test waits for has happened, as a host's loop turns: each
//! turn runs what is ready (a frame asked for, the tasks woken), and between turns the test waits
//! for a task to wake the app (Waft's `TestPlatform::wait_for_wake`), never on a timer.
//!
//! The invariant a later editor will break: only a turn that leaves something to turn for (a
//! frame, a window, a timer) is followed at once; any other waits for a wake. So what a test waits
//! for must reach the app through one of its tasks, as everything the app shows does, or wake the
//! host itself, as a program's output and its end do here (`Lines::wake`, [`wake_once`]); a
//! change that wakes nothing fails at [`WITHIN`].

use std::sync::Arc;
use std::thread;
use std::time::Instant;

use waft::ui::Dispatcher;
use waft_test::TestPlatform;

use crate::logic::test_engine::WITHIN;

/// What one turn of a test's app left.
pub(crate) enum Turn {
    /// What the test waits for has happened.
    Done,
    /// A frame is asked for: the next turn draws it.
    FrameAsked,
    /// Nothing until a task wakes the app.
    Idle,
}

/// Turns the app with `turn` until it answers [`Turn::Done`], waiting on `platform` for a wake
/// after each idle turn; fails after [`WITHIN`] with `what` was awaited.
#[track_caller]
pub(crate) fn turn_until(platform: &TestPlatform, what: &str, mut turn: impl FnMut() -> Turn) {
    let deadline = Instant::now() + WITHIN;
    loop {
        let wakes = platform.wake_now_requests();
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "never within {WITHIN:?}: {what}");
        match turn() {
            Turn::Done => return,
            Turn::FrameAsked => {}
            Turn::Idle => platform.wait_for_wake(wakes, left),
        }
    }
}

/// Waits for `event` on a thread of its own, then wakes `host`, as a test's host hears of it: for
/// a test turning its app until what the engine announces with `event`, as a program ending, has
/// happened.
pub(crate) fn wake_once(event: impl Future + Send + 'static, host: Arc<dyn Dispatcher>) {
    thread::spawn(move || {
        futures::executor::block_on(event);
        host.wake_now();
    });
}
