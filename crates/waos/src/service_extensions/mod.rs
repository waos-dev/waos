//! `wasdk:platform/service-extensions`: the named calls a development tool makes of a running
//! program, each answered with JSON, as Flutter's service extensions are, which the program's
//! framework serves from its table of them (Waft's `ext.waft.*`).
//!
//! The program listens through [`ProgramServiceExtensions::calls`], each call a stream of its own.
//! The shell calls through [`ProgramServiceExtensions::call`], which hands the call to the stream
//! opened last that is still open, and resolves with the program's answer, the program thawed
//! until it has answered, as it is while it owes an answer to a request to end; with none open, it
//! answers `unknown` with no names at once. A call dropped without an answer answers `failed`.
//! Whether a program serves extensions now, for a tool's list of them, is
//! [`ProgramServiceExtensions::serves`]. The interface is served to a program by `host`, to its
//! main instance alone: a worker's stream ends at once.
//!
//! The invariant a later editor must keep: every call is answered exactly once, by the program's
//! `respond`, by `failed` when it drops the call or its stream goes before it reads it, or by
//! `unknown` when no stream is open, so a tool never waits for a call no program holds.

pub(crate) mod host;

use std::future::Future;
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::channel::{mpsc, oneshot};

pub use crate::bindings::wasdk::platform::service_extensions::ExtensionError;
use crate::lifecycle::Lifecycle;
use crate::sync::locked;

/// What a call is answered with: the answer's JSON text, or why there is none.
pub type ExtensionAnswer = Result<String, ExtensionError>;

/// Compared whole by whoever checks an answer, as a tool's test does.
impl PartialEq for ExtensionError {
    fn eq(&self, other: &ExtensionError) -> bool {
        match (self, other) {
            (ExtensionError::Unknown(names), ExtensionError::Unknown(others)) => names == others,
            (ExtensionError::InvalidParams(why), ExtensionError::InvalidParams(other))
            | (ExtensionError::Failed(why), ExtensionError::Failed(other)) => why == other,
            _ => false,
        }
    }
}

/// The calls one of the program's streams receives.
pub(crate) type ExtensionCalls = mpsc::UnboundedReceiver<ExtensionCall>;

/// Why a call the program let go of, without answering, has no answer.
const LET_GO: &str = "the program let go of the call without answering it";

/// One program's service extensions, as the shell calls them and the program listens for them.
/// Every clone reaches the same one.
#[derive(Clone)]
pub struct ProgramServiceExtensions {
    streams: Arc<Mutex<Vec<mpsc::UnboundedSender<ExtensionCall>>>>,
    /// The program's lifecycle, which a call keeps it thawed in until it is answered.
    lifecycle: Lifecycle,
}

/// One call, as one stream receives it, answered once with [`respond`](Self::respond). Dropped
/// without an answer, it answers `failed`.
pub struct ExtensionCall {
    name: String,
    params: Vec<(String, String)>,
    reply: oneshot::Sender<ExtensionAnswer>,
}

impl ProgramServiceExtensions {
    /// The service extensions of the program whose lifecycle is `lifecycle`.
    pub(crate) fn new(lifecycle: Lifecycle) -> ProgramServiceExtensions {
        ProgramServiceExtensions {
            streams: Arc::default(),
            lifecycle,
        }
    }

    /// A stream of its own for the calls from now on.
    pub(crate) fn calls(&self) -> ExtensionCalls {
        let (stream, calls) = mpsc::unbounded();
        locked(&self.streams).push(stream);
        calls
    }

    /// Calls the extension `name` with `params`: resolves with the program's answer, once it
    /// answers, thawed until it has; at once with `unknown` and no names when it has no stream
    /// open; and with `failed` when it lets go of the call without answering.
    pub fn call(
        &self,
        name: &str,
        params: Vec<(String, String)>,
    ) -> impl Future<Output = ExtensionAnswer> + Send + 'static + use<> {
        let (reply, answer) = oneshot::channel();
        let call = ExtensionCall {
            name: name.to_owned(),
            params,
            reply,
        };
        let owed = self.lifecycle.owe_answer();
        let sent = self.send_to_newest(call);
        async move {
            if !sent {
                return Err(ExtensionError::Unknown(Vec::new()));
            }
            let answer = answer
                .map(|answer| answer.unwrap_or_else(|_| Err(ExtensionError::Failed(LET_GO.into()))))
                .await;
            drop(owed);
            answer
        }
    }

    /// Whether the program serves extensions now: it has a stream of calls open.
    pub fn serves(&self) -> bool {
        let mut streams = locked(&self.streams);
        streams.retain(|stream| !stream.is_closed());
        !streams.is_empty()
    }

    /// Hands `call` to the stream opened last that is still open; whether one took it.
    fn send_to_newest(&self, mut call: ExtensionCall) -> bool {
        let mut streams = locked(&self.streams);
        streams.retain(|stream| !stream.is_closed());
        while let Some(newest) = streams.last() {
            match newest.unbounded_send(call) {
                Ok(()) => return true,
                Err(refused) => {
                    call = refused.into_inner();
                    streams.pop();
                }
            }
        }
        false
    }
}

impl ExtensionCall {
    /// The extension's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The call's parameters, each its name and its value, in the order given.
    pub(crate) fn params(&self) -> &[(String, String)] {
        &self.params
    }

    /// Answers the call.
    pub(crate) fn respond(self, answer: ExtensionAnswer) {
        // Nobody waits for it once the shell has let go of the call.
        let _ = self.reply.send(answer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn received(calls: &mut ExtensionCalls) -> ExtensionCall {
        calls.try_recv().expect("a call was sent")
    }

    fn params() -> Vec<(String, String)> {
        vec![("objectGroup".to_owned(), "tree".to_owned())]
    }

    #[test]
    fn a_call_reaches_the_program_and_is_answered_with_its_json() {
        let extensions = ProgramServiceExtensions::new(Lifecycle::default());
        let mut calls = extensions.calls();
        assert!(extensions.serves());
        let answer = extensions.call("ext.waft.debugDumpApp", params());
        let call = received(&mut calls);
        assert_eq!(
            (call.name(), call.params()),
            ("ext.waft.debugDumpApp", &params()[..])
        );
        call.respond(Ok(r#"{"tree":"…"}"#.to_owned()));
        assert_eq!(
            answer.now_or_never(),
            Some(Ok(r#"{"tree":"…"}"#.to_owned()))
        );
    }

    #[test]
    fn with_no_stream_open_a_call_is_unknown_with_no_names_at_once() {
        let extensions = ProgramServiceExtensions::new(Lifecycle::default());
        assert!(!extensions.serves());
        let answer = extensions.call("ext.waft.debugDumpApp", Vec::new());
        assert_eq!(
            answer.now_or_never(),
            Some(Err(ExtensionError::Unknown(Vec::new())))
        );
        let calls = extensions.calls();
        drop(calls);
        assert!(!extensions.serves(), "a stream dropped serves nothing");
        let answer = extensions.call("ext.waft.debugDumpApp", Vec::new());
        assert_eq!(
            answer.now_or_never(),
            Some(Err(ExtensionError::Unknown(Vec::new())))
        );
    }

    #[test]
    fn a_call_dropped_without_an_answer_answers_failed() {
        let extensions = ProgramServiceExtensions::new(Lifecycle::default());
        let mut calls = extensions.calls();
        let answer = extensions.call("ext.waft.debugDumpApp", Vec::new());
        drop(received(&mut calls));
        assert!(matches!(
            answer.now_or_never(),
            Some(Err(ExtensionError::Failed(_)))
        ));
        let unread = extensions.call("ext.waft.debugDumpApp", Vec::new());
        drop(calls);
        assert!(
            matches!(unread.now_or_never(), Some(Err(ExtensionError::Failed(_)))),
            "a call on a stream dropped before reading it answers failed too"
        );
    }

    #[test]
    fn a_call_reaches_the_stream_opened_last_that_is_open() {
        let extensions = ProgramServiceExtensions::new(Lifecycle::default());
        let mut first = extensions.calls();
        let second = extensions.calls();
        let mut third = extensions.calls();
        let _answer = extensions.call("ext.one", Vec::new());
        assert!(third.try_recv().is_ok(), "the newest");
        drop(third);
        drop(second);
        let _answer = extensions.call("ext.two", Vec::new());
        assert_eq!(received(&mut first).name(), "ext.two");
    }
}
