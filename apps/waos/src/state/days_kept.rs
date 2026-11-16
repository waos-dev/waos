//! Keeping what each program did as the app goes to the background, as a native app saves on
//! Flutter's `paused`: a phone ends a backgrounded app without asking it, so the days counted
//! since the engine's writer last kept them would go with it.
//!
//! The invariant: only `paused` keeps them, which a desktop never reports, so switching windows,
//! hiding WaOS or covering it writes nothing; and the keeping runs off the main thread, since it
//! writes files.

use std::rc::Rc;
use std::sync::Arc;

use waft::{App, AppLifecycleListener};
use waos::WaOS;

/// From now on, every time the app goes to the background, the days `engine` counted are kept.
pub(crate) fn keep_every_day_when_paused(app: &mut App, engine: Arc<WaOS>) {
    let keep = move |app: &mut App| {
        let engine = Arc::clone(&engine);
        app.spawn(async move |cx| {
            cx.run_in_background(move || engine.keep_every_day()).await;
        });
    };
    AppLifecycleListener::new(app).on_pause(app, Rc::new(keep));
}
