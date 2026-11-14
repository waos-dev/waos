//! What a program put on the clipboard, while it is there: its `source`, which hears a request each
//! time someone pastes one of its promised forms or files, and the requests' answers (`answers`).
//!
//! A paste waits for the program's answer as the platform's own pastes wait for a promised form:
//! the program is thawed while it owes one; a form's paste fails once [`PROMISE_DEADLINE`] has
//! passed without the whole of it, as WinRT's data provider deadline fails one, and a file's once
//! it has passed without the program beginning to answer, after which the file is written as the
//! program streams it, as `NSFilePromiseProvider` waits for its writer; and a request dropped
//! unanswered, or an answer whose `done` does not say ok, fails it at once. When the program
//! flushes the clipboard, and before the engine stops it, the program is asked for every form its
//! source still promises, files aside ([`ProgramSources::keep_promised`]), and what it gives in
//! time answers every later paste without it.
//!
//! The invariant: a source's requests end once another copy has replaced it, by the clipboard's
//! count of changes, which only the shell's word moves; and a paste of a form kept never reaches
//! the program, which may have ended.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::future::{self, Either, join_all};
use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::watch;

use super::answers::{self, Answer, Answering};
use super::{Listing, PlatformClipboard};
use crate::lifecycle::Lifecycle;
use crate::shell::{ContentFuture, FileSink, FileWriteFuture};
use crate::sync::locked;

/// How long a paste waits for a program's promised form, or for it to begin a promised file,
/// before it fails, as WinRT's data provider deadline fails one.
pub(crate) const PROMISE_DEADLINE: Duration = Duration::from_secs(5);

/// One promised form or file: which item, by its place in what was written, and which of its
/// types.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PromisedForm {
    pub(crate) item: u32,
    pub(crate) mime: String,
}

/// What a program put on the clipboard: the value behind its `source`. Every clone is the same.
#[derive(Clone)]
pub struct Source(Arc<SourceState>);

struct SourceState {
    /// The clipboard's change the copy is; set once the shell has written it.
    change: OnceLock<u64>,
    platform: PlatformClipboard,
    /// The forms it promises, files aside: what a flush or a stop asks for.
    forms: Vec<PromisedForm>,
    requests: mpsc::UnboundedSender<Request>,
    /// Where the requests arrive, until the program takes them.
    arriving: Mutex<Option<mpsc::UnboundedReceiver<Request>>>,
    /// What the program gave for a form before it ended, for every paste after.
    kept: Mutex<HashMap<PromisedForm, Vec<u8>>>,
    lifecycle: Lifecycle,
}

impl Source {
    /// A copy promising `forms`, files aside, of the program whose `lifecycle` thaws it to answer.
    pub(crate) fn new(
        platform: PlatformClipboard,
        forms: Vec<PromisedForm>,
        lifecycle: Lifecycle,
    ) -> Source {
        let (requests, arriving) = mpsc::unbounded();
        Source(Arc::new(SourceState {
            change: OnceLock::new(),
            platform,
            forms,
            requests,
            arriving: Mutex::new(Some(arriving)),
            kept: Mutex::default(),
            lifecycle,
        }))
    }

    /// The copy is on the clipboard as its change `change`.
    pub(crate) fn written_as(&self, change: u64) {
        let _ = self.0.change.set(change);
    }

    /// Whether another copy has replaced this one.
    fn replaced(&self, listing: &Listing) -> bool {
        self.0
            .change
            .get()
            .is_some_and(|change| listing.change > *change)
    }

    /// The requests for the promised forms, until another copy replaces this one; the first call
    /// takes them, and a later one has none.
    pub(crate) fn requests(&self) -> BoxStream<'static, Request> {
        let Some(arriving) = locked(&self.0.arriving).take() else {
            return stream::empty().boxed();
        };
        let watch = RequestsWatch {
            source: self.clone(),
            arriving,
            listing: self.0.platform.changes(),
        };
        stream::unfold(watch, RequestsWatch::next_request).boxed()
    }

    /// What produces `form` each time someone pastes it.
    pub(crate) fn producer(
        &self,
        form: PromisedForm,
    ) -> impl Fn() -> ContentFuture + Send + Sync + use<> {
        let source = self.clone();
        move || Box::pin(source.clone().produce(form.clone()))
    }

    /// What produces the promised file `form` into the sink a paste hands it.
    pub(crate) fn file_producer(
        &self,
        form: PromisedForm,
    ) -> impl Fn(Box<dyn FileSink>) -> FileWriteFuture + Send + Sync + use<> {
        let source = self.clone();
        move |sink| Box::pin(source.clone().produce_file(form.clone(), sink))
    }

    /// `form`'s content: kept, or asked of the program, which is thawed while it owes it; none
    /// when it gave nothing whole in time.
    async fn produce(self, form: PromisedForm) -> Option<Vec<u8>> {
        if let Some(kept) = locked(&self.0.kept).get(&form) {
            return Some(kept.clone());
        }
        let _owed = self.0.lifecycle.owe_answer();
        let answer = self.ask(form)?;
        within(PROMISE_DEADLINE, answer.whole()).await.flatten()
    }

    /// The promised file `form`, written into `sink` as the program streams it, once it begins
    /// to answer in time; the program is thawed until the file is whole.
    async fn produce_file(self, form: PromisedForm, mut sink: Box<dyn FileSink>) -> io::Result<()> {
        let _owed = self.0.lifecycle.owe_answer();
        let Some(mut answer) = self.ask(form) else {
            return Err(io::Error::other("the program has ended"));
        };
        match within(PROMISE_DEADLINE, answer.begun()).await {
            Some(true) => answer.written_into(sink.as_mut()).await,
            Some(false) => Err(io::Error::other("the program dropped the request")),
            None => Err(io::Error::other("the program did not answer in time")),
        }
    }

    /// Asks the program for `form`: the paste's side of its answer; none once the program can
    /// hear no request.
    fn ask(&self, form: PromisedForm) -> Option<Answer> {
        let (answering, answer) = answers::answer();
        let request = Request(Arc::new(RequestState {
            form,
            answering: Mutex::new(Some(answering)),
        }));
        self.0.requests.unbounded_send(request).ok()?;
        Some(answer)
    }

    /// Asks the program for every form not kept yet, files aside, and keeps what it gives in
    /// time.
    async fn keep_promised(self) {
        let unkept: Vec<PromisedForm> = {
            let kept = locked(&self.0.kept);
            let forms = self.0.forms.iter();
            forms
                .filter(|form| !kept.contains_key(form))
                .cloned()
                .collect()
        };
        let given = unkept.into_iter().map(|form| {
            let source = self.clone();
            async move { (form.clone(), source.produce(form).await) }
        });
        for (form, content) in join_all(given).await {
            if let Some(content) = content {
                locked(&self.0.kept).insert(form, content);
            }
        }
    }
}

/// The requests a program reads, until its copy is replaced.
struct RequestsWatch {
    source: Source,
    arriving: mpsc::UnboundedReceiver<Request>,
    listing: watch::Receiver<Listing>,
}

impl RequestsWatch {
    async fn next_request(mut self) -> Option<(Request, RequestsWatch)> {
        loop {
            if self.source.replaced(&self.listing.borrow_and_update()) {
                return None;
            }
            tokio::select! {
                request = self.arriving.next() => return Some((request?, self)),
                changed = self.listing.changed() => changed.ok()?,
            }
        }
    }
}

/// One paste of a promised form or file: the value behind a program's `request`. Every clone is
/// the same.
#[derive(Clone)]
pub struct Request(Arc<RequestState>);

struct RequestState {
    form: PromisedForm,
    /// Taken by the answer; dropped unanswered, it fails the paste.
    answering: Mutex<Option<Answering>>,
}

impl Request {
    pub(crate) fn form(&self) -> &PromisedForm {
        &self.0.form
    }

    /// The program's side of the answer, which its content and `done` are piped into; none once
    /// the request was answered.
    pub(crate) fn answer(&self) -> Option<Answering> {
        locked(&self.0.answering).take()
    }
}

/// A program's copies: the last one it put on the clipboard, whose forms it is asked for when it
/// flushes the clipboard and before the engine stops it. Every clone is the same.
#[derive(Clone, Default)]
pub(crate) struct ProgramSources(Arc<Mutex<Option<Source>>>);

impl ProgramSources {
    /// The program put `source` on the clipboard.
    pub(crate) fn wrote(&self, source: Source) {
        *locked(&self.0) = Some(source);
    }

    /// Asks the program for every form its last copy still promises, files aside, if nothing has
    /// replaced it, and keeps what it gives in time; resolves once each is given or has timed out.
    pub(crate) fn keep_promised(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let source = locked(&self.0).clone();
        async move {
            let Some(source) = source else {
                return;
            };
            if source.replaced(&source.0.platform.listing()) {
                return;
            }
            source.keep_promised().await;
        }
    }
}

/// `future`'s output, or none once `limit` has passed. A thread of its own keeps the time, since
/// a paste waits on whichever thread the shell runs it, with no timer of the engine's there.
async fn within<T>(limit: Duration, future: impl Future<Output = T>) -> Option<T> {
    let (elapsed, timed_out) = oneshot::channel::<()>();
    let timer = thread::Builder::new()
        .name("clipboard deadline".to_owned())
        .spawn(move || {
            thread::sleep(limit);
            let _ = elapsed.send(());
        });
    if timer.is_err() {
        return None;
    }
    match future::select(pin!(future), timed_out).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(_) => None,
    }
}

#[cfg(test)]
mod tests;
