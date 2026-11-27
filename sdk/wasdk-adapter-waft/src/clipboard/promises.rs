//! A write's promises, answered: each paste of a promised form or file is a request of the write's
//! source, answered in a task of the program's own by the producer Waft gave, with the content and
//! whether it was made whole, the WIT's `done`. A promised file is produced into a sink that writes
//! the request's stream as the bytes come.
//!
//! The invariant: a write's promises are answered only from the requests of the source that write
//! answered, which end once another copy replaces it, so a paste never reaches the producer of a
//! copy that is gone; and a request's `done` is written only once its stream has ended.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;

use waft_embedder::{ContentFuture, FileSink, WriteFuture};
use wasdk::clipboard::{Request, Source};
use wasdk::{FutureReader, StreamWriter, wit_future, wit_stream};

use super::stream_of;

/// What makes one promised form or file, each time someone pastes it.
#[derive(Clone)]
pub(super) enum Producer {
    Form(Rc<dyn Fn() -> ContentFuture>),
    File(Rc<dyn Fn(Box<dyn FileSink>) -> WriteFuture>),
}

/// What produces each promised form and file of a write, by its item's place and its type.
#[derive(Default)]
pub(super) struct Promises(HashMap<(u32, String), Producer>);

impl Promises {
    /// `producer` makes the form `mime` of the item at `item`; the first given for a form is kept.
    pub(super) fn promise(&mut self, item: u32, mime: String, producer: Producer) {
        self.0.entry((item, mime)).or_insert(producer);
    }

    /// What makes the form `mime` of the item at `item`.
    pub(super) fn producer(&self, item: u32, mime: String) -> Option<Producer> {
        self.0.get(&(item, mime)).cloned()
    }
}

/// Answers each paste of `source`'s promises, each in a task of its own, until another copy
/// replaces it.
pub(super) async fn answer_pastes(source: Source, promises: Promises) {
    let promises = Rc::new(promises);
    let mut requests = source.requests();
    while let Some(request) = requests.next().await {
        wasdk::spawn(answer(request, Rc::clone(&promises)));
    }
}

/// Answers one paste with what its producer makes.
async fn answer(request: Request, promises: Rc<Promises>) {
    match promises.producer(request.item(), request.type_()) {
        Some(Producer::Form(produce)) => answer_form(request, produce).await,
        Some(Producer::File(produce)) => answer_file(request, produce).await,
        None => {}
    }
}

/// Answers with the whole form, or, when the producer made none, with nothing and a `done` saying
/// so.
async fn answer_form(request: Request, produce: Rc<dyn Fn() -> ContentFuture>) {
    let (content, made) = match produce().await {
        Some(content) => (content, Ok(())),
        None => (Vec::new(), Err("the form was not made".to_owned())),
    };
    request.provide(stream_of(content), done_with(made));
}

/// Answers with the file's bytes as the producer writes them, then a `done` saying whether it
/// made the file whole; the stream ends with the producer, which holds its sink.
async fn answer_file(request: Request, produce: Rc<dyn Fn(Box<dyn FileSink>) -> WriteFuture>) {
    let (writer, content) = wit_stream::new::<u8>();
    let (done, settled) = wit_future::new::<Result<(), String>>(unanswered);
    request.provide(content, settled);
    let made = produce(Box::new(RequestStream(writer))).await;
    let _ = done.write(made.map_err(|error| error.to_string())).await;
}

/// A promised file's sink: the stream of the request it answers.
struct RequestStream(StreamWriter<u8>);

impl FileSink for RequestStream {
    fn write<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>> {
        Box::pin(async move {
            let unwritten = self.0.write_all(bytes.to_vec()).await;
            if unwritten.is_empty() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "the paste no longer reads the file",
                ))
            }
        })
    }
}

/// `made`, as the `done` the program gives, written by a task of its own.
fn done_with(made: Result<(), String>) -> FutureReader<Result<(), String>> {
    let (done, reader) = wit_future::new::<Result<(), String>>(unanswered);
    wasdk::spawn(async move {
        let _ = done.write(made).await;
    });
    reader
}

/// What a `done` dropped unwritten says.
fn unanswered() -> Result<(), String> {
    Err("dropped unanswered".to_owned())
}

#[cfg(test)]
mod tests {
    use std::future::ready;

    use super::*;

    fn form(bytes: &'static [u8]) -> Producer {
        Producer::Form(Rc::new(move || Box::pin(ready(Some(bytes.to_vec())))))
    }

    #[test]
    fn each_promise_is_made_by_its_own_producer_the_first_given_kept() {
        let mut promises = Promises::default();
        promises.promise(0, "text/html".to_owned(), form(b"<b>a</b>"));
        promises.promise(1, "text/html".to_owned(), form(b"<b>b</b>"));
        promises.promise(1, "text/html".to_owned(), form(b"<b>later</b>"));
        let file: Producer = Producer::File(Rc::new(|_| Box::pin(ready(Ok(())))));
        promises.promise(1, "image/png".to_owned(), file);
        let made = |item, mime: &str| match promises.producer(item, mime.to_owned()) {
            Some(Producer::Form(produce)) => futures::executor::block_on(produce()),
            _ => None,
        };
        assert_eq!(made(0, "text/html").as_deref(), Some(&b"<b>a</b>"[..]));
        assert_eq!(made(1, "text/html").as_deref(), Some(&b"<b>b</b>"[..]));
        assert_eq!(made(1, "text/plain"), None);
        assert!(matches!(
            promises.producer(1, "image/png".to_owned()),
            Some(Producer::File(_))
        ));
    }
}
