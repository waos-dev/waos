//! The platform asking a program whether it may end.
//!
//! The program listens through [`Lifecycle::exit_requests`], each call a stream of its own. The
//! engine asks through [`Lifecycle::request_exit`] when the shell's user asks the platform to
//! quit, and hears one answer for every stream once each has answered: cancel if any cancelled,
//! exit otherwise. A request dropped without an answer answers exit, and a stream dropped is not
//! asked, so a program that does not listen never keeps the platform from quitting.
//!
//! The invariants a later editor must keep: a request is decided only once every stream has
//! answered it, as Flutter's widgets binding asks every observer before it answers; and every
//! request reaches the streams, even while an earlier one is undecided, since the program decides
//! what a second quit means.
//!
//! [`Lifecycle::exit_requests`]: super::Lifecycle::exit_requests
//! [`Lifecycle::request_exit`]: super::Lifecycle::request_exit

use std::future::Future;

use futures::channel::{mpsc, oneshot};
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};

use crate::bindings::wasdk::platform::lifecycle::AppExitResponse;

/// The requests to end the program, as one of its streams receives them.
pub(crate) type ExitRequests = mpsc::UnboundedReceiver<ExitRequest>;

/// One request to end the program, as one stream receives it, answered once with
/// [`respond`](Self::respond). Dropped without an answer, it answers [`AppExitResponse::Exit`].
pub struct ExitRequest {
    reply: oneshot::Sender<AppExitResponse>,
}

/// The program's streams of exit requests.
#[derive(Default)]
pub(super) struct ExitListeners {
    /// One per stream the program opened; one it dropped is forgotten the next time the
    /// platform asks.
    streams: Vec<mpsc::UnboundedSender<ExitRequest>>,
}

impl ExitListeners {
    /// A stream of its own for the requests from now on.
    pub(super) fn listen(&mut self) -> ExitRequests {
        let (stream, requests) = mpsc::unbounded();
        self.streams.push(stream);
        requests
    }

    /// Asks every stream open.
    pub(super) fn ask(&mut self) -> impl Future<Output = AppExitResponse> + Send + use<> {
        answer_of_all(self.send_requests())
    }

    /// A request on every stream still open, and each one's answer on its way.
    fn send_requests(&mut self) -> Vec<impl Future<Output = AppExitResponse> + Send + use<>> {
        self.streams.retain(|stream| !stream.is_closed());
        self.streams
            .iter()
            .filter_map(|stream| {
                let (reply, answer) = oneshot::channel();
                stream.unbounded_send(ExitRequest { reply }).ok()?;
                Some(answer.map(|answer| answer.unwrap_or(AppExitResponse::Exit)))
            })
            .collect()
    }
}

impl ExitRequest {
    /// Answers the request.
    pub(crate) fn respond(self, response: AppExitResponse) {
        // Nobody waits for it once the engine has let go of the request.
        let _ = self.reply.send(response);
    }
}

/// The one answer of several asked at once, once every one has answered: cancel if any
/// cancelled, exit otherwise. How a platform asks everything that may refuse, as Flutter's
/// widgets binding asks every observer.
async fn answer_of_all<F>(answers: impl IntoIterator<Item = F>) -> AppExitResponse
where
    F: Future<Output = AppExitResponse>,
{
    let mut answers: FuturesUnordered<F> = answers.into_iter().collect();
    let mut did_cancel = false;
    while let Some(answer) = answers.next().await {
        if answer == AppExitResponse::Cancel {
            did_cancel = true;
            // No early return, as in Flutter's `handleRequestAppExit`: an answer still on its way
            // belongs to a request that is not decided until it arrives.
        }
    }
    if did_cancel {
        AppExitResponse::Cancel
    } else {
        AppExitResponse::Exit
    }
}

#[cfg(test)]
mod tests {
    use futures::future;

    use super::super::Lifecycle;
    use super::*;

    /// The request `requests` received, which must be there already.
    fn received(requests: &mut ExitRequests) -> ExitRequest {
        requests.try_recv().expect("a request was sent")
    }

    #[test]
    fn a_program_that_does_not_listen_may_end_at_once() {
        let lifecycle = Lifecycle::default();
        let answer = lifecycle.request_exit().now_or_never();
        assert_eq!(answer, Some(AppExitResponse::Exit));
    }

    #[test]
    fn every_stream_is_asked_and_every_one_must_answer_exit() {
        let lifecycle = Lifecycle::default();
        let (mut first, mut second) = (lifecycle.exit_requests(), lifecycle.exit_requests());
        let mut answer = lifecycle.request_exit().boxed();
        received(&mut first).respond(AppExitResponse::Exit);
        assert_eq!(
            (&mut answer).now_or_never(),
            None,
            "the second has not answered"
        );
        received(&mut second).respond(AppExitResponse::Exit);
        assert_eq!(answer.now_or_never(), Some(AppExitResponse::Exit));
    }

    #[test]
    fn a_cancel_decides_once_every_other_stream_has_answered() {
        let lifecycle = Lifecycle::default();
        let (mut first, mut second) = (lifecycle.exit_requests(), lifecycle.exit_requests());
        let mut answer = lifecycle.request_exit().boxed();
        let deciding = received(&mut first);
        received(&mut second).respond(AppExitResponse::Cancel);
        assert_eq!(
            (&mut answer).now_or_never(),
            None,
            "the first is still deciding"
        );
        deciding.respond(AppExitResponse::Exit);
        assert_eq!(answer.now_or_never(), Some(AppExitResponse::Cancel));
    }

    #[test]
    fn a_request_dropped_unanswered_answers_exit() {
        let lifecycle = Lifecycle::default();
        let mut requests = lifecycle.exit_requests();
        let answer = lifecycle.request_exit();
        drop(received(&mut requests));
        assert_eq!(answer.now_or_never(), Some(AppExitResponse::Exit));
    }

    #[test]
    fn a_stream_the_program_dropped_is_not_asked() {
        let lifecycle = Lifecycle::default();
        drop(lifecycle.exit_requests());
        let answer = lifecycle.request_exit().now_or_never();
        assert_eq!(answer, Some(AppExitResponse::Exit));
    }

    #[test]
    fn a_request_while_one_is_undecided_reaches_the_program_and_is_decided_alone() {
        let lifecycle = Lifecycle::default();
        let mut requests = lifecycle.exit_requests();
        let first = lifecycle.request_exit();
        let second = lifecycle.request_exit();
        let (first_request, second_request) = (received(&mut requests), received(&mut requests));
        second_request.respond(AppExitResponse::Exit);
        first_request.respond(AppExitResponse::Cancel);
        let answers = future::join(first, second).now_or_never();
        assert_eq!(
            answers,
            Some((AppExitResponse::Cancel, AppExitResponse::Exit))
        );
    }

    #[test]
    fn a_request_after_a_decision_asks_again() {
        let lifecycle = Lifecycle::default();
        let mut requests = lifecycle.exit_requests();
        let first = lifecycle.request_exit();
        received(&mut requests).respond(AppExitResponse::Cancel);
        assert_eq!(first.now_or_never(), Some(AppExitResponse::Cancel));
        let second = lifecycle.request_exit();
        received(&mut requests).respond(AppExitResponse::Exit);
        assert_eq!(second.now_or_never(), Some(AppExitResponse::Exit));
    }

    #[test]
    fn the_answer_of_all_is_exit_only_when_every_one_is() {
        let all_exit = [AppExitResponse::Exit, AppExitResponse::Exit].map(future::ready);
        assert_eq!(
            answer_of_all(all_exit).now_or_never(),
            Some(AppExitResponse::Exit)
        );
        let one_cancel = [AppExitResponse::Exit, AppExitResponse::Cancel].map(future::ready);
        assert_eq!(
            answer_of_all(one_cancel).now_or_never(),
            Some(AppExitResponse::Cancel)
        );
    }

    #[test]
    fn the_answer_of_all_waits_for_the_last_answer_after_a_cancel() {
        let (reply, later) = oneshot::channel();
        let answers = [
            future::ready(AppExitResponse::Cancel).boxed(),
            later
                .map(|answer| answer.unwrap_or(AppExitResponse::Exit))
                .boxed(),
        ];
        let mut answer = answer_of_all(answers).boxed();
        assert_eq!((&mut answer).now_or_never(), None);
        reply.send(AppExitResponse::Exit).unwrap();
        assert_eq!(answer.now_or_never(), Some(AppExitResponse::Cancel));
    }
}
