//! Ending the program, both ways: the platform asking whether it may (the `lifecycle` interface's
//! exit requests), which the framework answers through `EmbedderClient::request_app_exit`, and the
//! app ending itself (`AppExit::quit`), as a required exit does at once and a cancelable one once
//! the framework's listeners agree.
//!
//! A program ends by returning from `main`, so quitting tells the platform nothing: `quit` only
//! sends the loop a notice, and the loop ends on its next turn, which ends `run` and the program.
//!
//! The app quits too when the platform takes its last view away, as a card swiped away in
//! Android's Recents does, through the same quit (`quit_without_views`): as Flutter's Android
//! embedding destroys an engine of its own, and the app with it, when its activity is destroyed.
//! A request to close a view is the app's to answer, as before.

use futures::channel::mpsc;
use waft_embedder::{AppExit, AppExitReply, AppExitResponse};
use wasdk::lifecycle::{self, ExitRequest};

use crate::events::Notice;

/// Where the app's quit goes: the loop's notices.
pub(crate) struct WasdkAppExit {
    notices: mpsc::UnboundedSender<Notice>,
}

impl WasdkAppExit {
    pub(crate) fn new(notices: mpsc::UnboundedSender<Notice>) -> WasdkAppExit {
        WasdkAppExit { notices }
    }
}

impl AppExit for WasdkAppExit {
    /// Ends the loop on its next turn. The exit code is ignored, as Flutter's macOS and Linux
    /// embedders ignore it: a program's `main` reports only whether it failed.
    fn quit(&self, _exit_code: i32) {
        let _ = self.notices.unbounded_send(Notice::Quit);
    }
}

/// The platform took a view away and `views_left` remain: with none, the app quits through
/// `exit`, as it quits itself.
pub(crate) fn quit_without_views(exit: &dyn AppExit, views_left: usize) {
    if views_left == 0 {
        exit.quit(0);
    }
}

/// The reply the framework answers the platform's `request` with. Answering it calls the
/// platform, never the client, so it may be answered inside a call into the client.
pub(crate) fn reply_to(request: ExitRequest) -> AppExitReply {
    AppExitReply::new(move |response| ExitRequest::respond(request, platform_response(response)))
}

/// The framework's answer as the platform's.
fn platform_response(response: AppExitResponse) -> lifecycle::AppExitResponse {
    match response {
        AppExitResponse::Exit => lifecycle::AppExitResponse::Exit,
        AppExitResponse::Cancel => lifecycle::AppExitResponse::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quit_waits_for_the_loop() {
        let (sender, mut notices) = mpsc::unbounded();
        WasdkAppExit::new(sender).quit(0);
        assert!(matches!(notices.try_recv(), Ok(Notice::Quit)));
    }

    #[test]
    fn the_app_quits_once_the_platform_took_its_last_view() {
        let (sender, mut notices) = mpsc::unbounded();
        let exit = WasdkAppExit::new(sender);
        quit_without_views(&exit, 1);
        assert!(notices.try_recv().is_err(), "a view remains");
        quit_without_views(&exit, 0);
        assert!(matches!(notices.try_recv(), Ok(Notice::Quit)));
    }

    #[test]
    fn the_framework_answer_reaches_the_platform_unchanged() {
        assert_eq!(
            platform_response(AppExitResponse::Exit),
            lifecycle::AppExitResponse::Exit
        );
        assert_eq!(
            platform_response(AppExitResponse::Cancel),
            lifecycle::AppExitResponse::Cancel
        );
    }
}
