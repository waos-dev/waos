//! Quitting the program's app asks the program, through WaOS (decision 12 of
//! `docs/proposals/2026-10-29-macos-apps.md`). Asked by macOS to quit (its Quit, the Dock's,
//! logging out), Waft's host asks the app, which may answer later; the window process asks WaOS,
//! which asks the program as its own Quit does, and answers with the program's answer once WaOS
//! sends it. Until WaOS welcomes the app there is no one to ask, and Waft's host quits it as it
//! would any app.
//!
//! The invariant a later editor will break: every quit asked is answered, by the program's answer
//! or, once the app ends, with exit, so macOS never waits on an app that cannot answer.

use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::oneshot;
use waft::ui::AppExitResponse as WaftExitResponse;
use waft::{App, Task, WidgetsBinding, WidgetsBindingObserver};
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos_window_messages::FromWindow;

use crate::connection::Connection;

/// The quits the user asked of the program's app, each waiting for the program's answer.
#[derive(Clone, Default)]
pub(crate) struct QuitsAsked(Rc<RefCell<Vec<oneshot::Sender<WaftExitResponse>>>>);

impl QuitsAsked {
    /// From now on, a quit Waft's host asks of the app goes to the program through
    /// `connection`.
    pub(crate) fn ask_through(&self, app: &mut App, connection: Rc<Connection>) {
        let asker = Rc::new(QuitAsker {
            connection,
            asked: self.clone(),
        });
        WidgetsBinding::instance(app).add_observer(app, asker);
    }

    /// The program answered: every quit asked hears its answer.
    pub(crate) fn answered(&self, response: AppExitResponse) {
        for asked in self.0.borrow_mut().drain(..) {
            let _ = asked.send(waft_response(response));
        }
    }

    /// The app ends: every quit asked hears exit.
    pub(crate) fn app_ends(&self) {
        self.answered(AppExitResponse::Exit);
    }

    /// A quit asked, answered by what `answered` sends.
    fn wait(&self) -> oneshot::Receiver<WaftExitResponse> {
        let (answer, answered) = oneshot::channel();
        self.0.borrow_mut().push(answer);
        answered
    }
}

/// What asks the program when Waft's host asks the app to quit.
struct QuitAsker {
    connection: Rc<Connection>,
    asked: QuitsAsked,
}

impl WidgetsBindingObserver for QuitAsker {
    fn did_request_app_exit(&self, app: &mut App) -> Task<WaftExitResponse> {
        let answered = self.asked.wait();
        self.connection.send(FromWindow::QuitRequested);
        app.spawn(async move |_cx| answered.await.unwrap_or(WaftExitResponse::Exit))
    }
}

/// The program's answer, as Waft's host takes it.
fn waft_response(response: AppExitResponse) -> WaftExitResponse {
    match response {
        AppExitResponse::Exit => WaftExitResponse::Exit,
        AppExitResponse::Cancel => WaftExitResponse::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    #[test]
    fn every_quit_asked_hears_the_programs_answer_and_exit_once_the_app_ends() {
        let quits = QuitsAsked::default();
        let first = quits.wait();
        let second = quits.wait();
        quits.answered(AppExitResponse::Cancel);
        assert_eq!(first.now_or_never(), Some(Ok(WaftExitResponse::Cancel)));
        assert_eq!(second.now_or_never(), Some(Ok(WaftExitResponse::Cancel)));

        let later = quits.wait();
        quits.app_ends();
        assert_eq!(later.now_or_never(), Some(Ok(WaftExitResponse::Exit)));
    }
}
