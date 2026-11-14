//! A view's frames in flight: presented by the program and not yet handed to the shell, while
//! the GPU draws them. A refresh serves a view with [`MOST_FRAMES_IN_FLIGHT`] in flight no frame,
//! as Flutter's `Animator` begins none while its `FramePipeline(2)` is full, so a program whose GPU
//! work is slower than its screen draws at the GPU's pace, with no more than two frames, and their
//! hardware buffers, waiting on it.
//!
//! The invariant: a frame is counted from the program's present until its hand-over is over, or
//! until it is dropped without one, so a frame lost on the way never holds the count.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many of a view's frames may be in flight at once: one being drawn by the GPU while the
/// program records the next, Flutter's pipeline depth.
pub(super) const MOST_FRAMES_IN_FLIGHT: usize = 2;

/// How many of a view's frames are in flight now.
#[derive(Default)]
pub(super) struct FramesInFlight(Arc<AtomicUsize>);

/// One frame in flight, counted until this is dropped.
pub(crate) struct FrameInFlight(Arc<AtomicUsize>);

impl FramesInFlight {
    /// One frame more, in flight until what is answered is dropped.
    pub(super) fn begin(&self) -> FrameInFlight {
        self.0.fetch_add(1, Ordering::SeqCst);
        FrameInFlight(Arc::clone(&self.0))
    }

    /// Whether as many frames are in flight as may be.
    pub(super) fn full(&self) -> bool {
        self.0.load(Ordering::SeqCst) >= MOST_FRAMES_IN_FLIGHT
    }
}

impl Drop for FrameInFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_frames_in_flight_are_as_many_as_may_be_until_one_lands() {
        let in_flight = FramesInFlight::default();
        let first = in_flight.begin();
        assert!(!in_flight.full());
        let _second = in_flight.begin();
        assert!(in_flight.full());
        drop(first);
        assert!(!in_flight.full(), "one handed over, another may go");
    }
}
