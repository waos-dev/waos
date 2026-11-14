//! Whether the user lets the app use the cameras: a question WaOS asks, over the app's window, the
//! first time the app opens a camera, and keeps the answer to. Waft's platform does not carry the
//! answer, so the app reads it from `wasdk`, as the program it is.

use waft::{App, Listener};
use wasdk::permissions::{self, State};

/// Whether the user has yet to be asked: opening a camera then puts the question to them.
pub(crate) fn is_unanswered() -> bool {
    permissions::camera().state() == State::NotAsked
}

/// Calls `allowed` each time the user comes to allow the cameras from now on, as they do in
/// WaOS's settings for the app after refusing.
pub(crate) fn follow_allowing(app: &mut App, allowed: Listener) {
    let mut changes = permissions::camera().changes();
    app.spawn(async move |cx| {
        while let Some(state) = changes.next().await {
            if state == State::Granted {
                cx.update(|app| allowed.call(app));
            }
        }
    });
}
