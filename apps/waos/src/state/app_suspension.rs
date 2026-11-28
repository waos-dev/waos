//! Telling the engine when the system suspends WaOS and resumes it, as Waft's app lifecycle
//! reports it: `paused` is the system having sent the app away, as a native app's
//! `didEnterBackground` and `onStop` are, and leaving `paused` is it coming back. A desktop never
//! reports `paused`, so there the engine is told nothing, and nothing is suspended.
//!
//! The invariant: the engine is told on the main thread as the state changes, before the system
//! takes the app's time away; and what waits for the GPU's work in flight runs off the main
//! thread, since it blocks.

use std::rc::Rc;
use std::sync::Arc;

use waft::{App, AppLifecycleListener};
use waos::WaOS;

/// From now on, `engine` is suspended as the system suspends the app and resumed as it resumes
/// it.
pub(crate) fn follow_app_suspension(app: &mut App, engine: Arc<WaOS>) {
    let suspend = {
        let engine = Arc::clone(&engine);
        move |app: &mut App| {
            let gpu_work_done = engine.suspend();
            app.spawn(async move |cx| {
                cx.run_in_background(move || futures::executor::block_on(gpu_work_done))
                    .await;
            });
        }
    };
    let resume = move |_: &mut App| engine.resume();
    AppLifecycleListener::new(app)
        .on_pause(app, Rc::new(suspend))
        .on_restart(app, Rc::new(resume));
}
