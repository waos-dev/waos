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
use std::sync::atomic::{AtomicBool, Ordering};
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
/// a test turning its app until what the engine announces with `event` (a program ending, or
/// frozen) has happened. Answers whether it has: set before the wake, so the turn it wakes reads
/// it set, however briefly what `event` waited for held.
pub(crate) fn wake_once(
    event: impl Future + Send + 'static,
    host: Arc<dyn Dispatcher>,
) -> Arc<AtomicBool> {
    let happened = Arc::new(AtomicBool::new(false));
    let announced = Arc::clone(&happened);
    thread::spawn(move || {
        futures::executor::block_on(event);
        announced.store(true, Ordering::Release);
        host.wake_now();
    });
    happened
}
