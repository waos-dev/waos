//! How many of a video's frames the app drew itself: the embedder's events under its
//! `targets::VIDEO_FRAMES`, one for each frame it takes from a reader to draw, counted by a
//! `tracing` subscriber of the program's own.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use wasdk_adapter_waft::targets;

/// The time counted, in nanoseconds: ten seconds.
const COUNTED: u64 = 10_000_000_000;

/// The frames drawn so far.
#[derive(Clone, Default)]
pub(crate) struct FrameCount(Arc<AtomicUsize>);

impl FrameCount {
    /// A count of the frames drawn from now on, as the program's one subscriber.
    pub(crate) fn install() -> FrameCount {
        let count = FrameCount::default();
        let installed = tracing::subscriber::set_global_default(count.clone());
        if installed.is_err() {
            eprintln!("the frames drawn are not counted: the program has a subscriber already");
        }
        count
    }

    /// Says how many frames were drawn once ten seconds have passed.
    pub(crate) async fn say_after_ten_seconds(self) {
        wasdk::monotonic_clock::wait_for(COUNTED).await;
        let drawn = self.0.load(Ordering::Relaxed);
        println!("drew {drawn} video frames in the first ten seconds");
    }
}

impl Subscriber for FrameCount {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == targets::VIDEO_FRAMES
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, _event: &Event<'_>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}
