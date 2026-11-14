//! The rule of quitting WaOS while programs run. Each quit asks every running program whether it
//! may end, as Flutter's `System.requestAppExit` asks an app, waits until every one has answered,
//! as Flutter's widgets binding waits for every observer, and agrees only if every one did. The
//! programs answer later, each after its own save dialog if it has one, and with no time limit:
//! a limit that answered exit would end a program whose user is still deciding, and one that
//! answered cancel would keep WaOS open all the same. A quit asked while the programs decide an
//! earlier one asks them again, and each program decides what a second quit means, as Chrome's
//! "press again to quit" does.
//!
//! What a program that does not listen answers, or one that ends while asked, is the engine's:
//! exit.

use std::future::Future;

use futures::FutureExt;
use futures::future::join_all;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;

/// Whether every program agreed to end, once every one of `answers` has come: true if every one
/// is exit, false if any is cancel.
pub(crate) fn every_program_agrees<F>(
    answers: impl IntoIterator<Item = F>,
) -> impl Future<Output = bool>
where
    F: Future<Output = AppExitResponse>,
{
    join_all(answers).map(|answers| {
        answers
            .into_iter()
            .all(|answer| answer == AppExitResponse::Exit)
    })
}

#[cfg(test)]
mod tests {
    use futures::channel::oneshot;
    use futures::future::{self, BoxFuture};

    use super::*;

    /// A program's answer on its way, and where the program gives it.
    fn deciding() -> (
        BoxFuture<'static, AppExitResponse>,
        oneshot::Sender<AppExitResponse>,
    ) {
        let (answer, answered) = oneshot::channel();
        let answer_of = |answer: Result<AppExitResponse, _>| answer.unwrap();
        (answered.map(answer_of).boxed(), answer)
    }

    #[test]
    fn with_no_program_running_waos_may_quit_at_once() {
        let none: [future::Ready<AppExitResponse>; 0] = [];
        assert_eq!(every_program_agrees(none).now_or_never(), Some(true));
    }

    #[test]
    fn waos_quits_once_every_program_agreed() {
        let (first, first_answers) = deciding();
        let (second, second_answers) = deciding();
        let mut agreed = every_program_agrees([first, second]).boxed();
        first_answers.send(AppExitResponse::Exit).unwrap();
        assert_eq!((&mut agreed).now_or_never(), None, "one has not answered");
        second_answers.send(AppExitResponse::Exit).unwrap();
        assert_eq!(agreed.now_or_never(), Some(true));
    }

    #[test]
    fn one_program_refusing_keeps_waos_once_every_one_has_answered() {
        let (refusing, refusal) = deciding();
        let (still_deciding, decision) = deciding();
        let mut agreed = every_program_agrees([refusing, still_deciding]).boxed();
        refusal.send(AppExitResponse::Cancel).unwrap();
        assert_eq!(
            (&mut agreed).now_or_never(),
            None,
            "the second is still deciding"
        );
        decision.send(AppExitResponse::Exit).unwrap();
        assert_eq!(agreed.now_or_never(), Some(false));
    }

    #[test]
    fn two_quits_are_decided_each_on_its_own() {
        let (first, first_answer) = deciding();
        let (second, second_answer) = deciding();
        let first_quit = every_program_agrees([first]);
        let second_quit = every_program_agrees([second]);
        first_answer.send(AppExitResponse::Cancel).unwrap();
        second_answer.send(AppExitResponse::Exit).unwrap();
        assert_eq!(
            future::join(first_quit, second_quit).now_or_never(),
            Some((false, true))
        );
    }
}
