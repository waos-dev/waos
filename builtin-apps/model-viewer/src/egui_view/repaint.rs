//! A frame asked for later: egui's repaint after a delay, as a tooltip's wait or a caret's blink
//! asks, on a view whose frames come only when asked for.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use wasdk::monotonic_clock;
use wasdk_wgpu::SharedView;

/// The earliest moment a frame is due to be asked for, on the monotonic clock; none while no
/// wait is under way.
#[derive(Default)]
pub(super) struct RepaintTimer {
    due: Rc<Cell<Option<u64>>>,
}

impl RepaintTimer {
    /// Asks `view` for a frame once `delay` has passed, unless a wait that ends sooner is under
    /// way already.
    pub(super) fn request_frame_after(&self, view: &SharedView, delay: Duration) {
        let delay = u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX);
        let due = monotonic_clock::now().saturating_add(delay);
        if self.due.get().is_some_and(|pending| pending <= due) {
            return;
        }
        self.due.set(Some(due));
        let (pending, view) = (self.due.clone(), view.clone());
        wasdk::spawn(async move {
            monotonic_clock::wait_until(due).await;
            if pending.get() == Some(due) {
                pending.set(None);
            }
            view.request_frame();
        });
    }
}
