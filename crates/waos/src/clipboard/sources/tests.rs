use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use futures::executor::block_on;

use super::*;
use crate::clipboard::tests::{ListingShell, TestItem, now};

fn form(item: u32, mime: &str) -> PromisedForm {
    PromisedForm {
        item,
        mime: mime.to_owned(),
    }
}

/// A source promising `forms`, written as the clipboard's last change.
fn written_source(platform: &PlatformClipboard, forms: Vec<PromisedForm>) -> Source {
    let source = Source::new(platform.clone(), forms, Lifecycle::default());
    source.written_as(platform.set(vec![TestItem::offering(&["text/plain"])]));
    source
}

/// Answers `request` with `bytes`, as a program's stream that gives them and ends, its `done`
/// saying ok.
fn answer(request: &Request, bytes: &[u8]) {
    answer_ending(request, &[bytes], Some(Ok(())));
}

/// Answers `request` with `chunks`, then `done`, or none.
fn answer_ending(request: &Request, chunks: &[&[u8]], done: Option<answers::Done>) {
    let answering = request.answer().expect("the request is unanswered");
    answering.give(chunks, done);
}

/// A shell's sink, keeping what is written into it.
#[derive(Clone, Default)]
struct KeptFile(Arc<Mutex<Vec<u8>>>);

impl FileSink for KeptFile {
    fn write<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Box::pin(std::future::ready(Ok(())))
    }
}

/// The request `requests` gives next, once the paste has asked.
fn next_request(requests: &mut BoxStream<'static, Request>) -> Request {
    now(requests.next())
        .flatten()
        .expect("the program hears the paste")
}

#[test]
fn a_paste_of_a_promised_form_is_a_request_the_program_answers() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, vec![form(0, "image/png")]);
    let mut requests = source.requests();
    let produce = source.producer(form(0, "image/png"));
    let mut pasted = Box::pin(produce());
    assert!(
        now(pasted.as_mut()).is_none(),
        "the paste waits for the program"
    );

    let request = now(requests.next())
        .flatten()
        .expect("the program hears the paste");
    assert_eq!(request.form(), &form(0, "image/png"));
    answer(&request, b"\x89PNG");
    assert_eq!(block_on(pasted), Some(b"\x89PNG".to_vec()));
}

#[test]
fn a_request_dropped_unanswered_fails_the_paste() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, vec![form(0, "text/html")]);
    let mut requests = source.requests();
    let produce = source.producer(form(0, "text/html"));
    let mut pasted = Box::pin(produce());
    assert!(
        now(pasted.as_mut()).is_none(),
        "the paste waits for the program"
    );
    let request = now(requests.next()).flatten().expect("a request");
    drop(request);
    assert_eq!(block_on(pasted), None);
}

#[test]
fn a_form_whose_done_fails_or_never_comes_fails_the_paste() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, vec![form(0, "text/rtf")]);
    let mut requests = source.requests();
    let produce = source.producer(form(0, "text/rtf"));
    for done in [Some(Err("no rtf here".to_owned())), None] {
        let mut pasted = Box::pin(produce());
        assert!(now(pasted.as_mut()).is_none());
        answer_ending(&next_request(&mut requests), &[b"{\\rtf1"], done);
        assert_eq!(block_on(pasted), None, "a part is never the whole");
    }
}

#[test]
fn a_promised_file_is_written_into_the_sink_as_it_comes_and_fails_unless_done_says_ok() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, Vec::new());
    let mut requests = source.requests();
    let produce = source.file_producer(form(1, "image/png"));

    let file = KeptFile::default();
    let mut pasted = produce(Box::new(file.clone()));
    assert!(
        now(pasted.as_mut()).is_none(),
        "the paste waits for the program"
    );
    let request = next_request(&mut requests);
    assert_eq!(request.form(), &form(1, "image/png"));
    answer_ending(&request, &[b"\x89P", b"NG"], Some(Ok(())));
    block_on(pasted).expect("the file is whole");
    assert_eq!(*file.0.lock().unwrap(), b"\x89PNG");

    let mut pasted = produce(Box::<KeptFile>::default());
    assert!(now(pasted.as_mut()).is_none());
    answer_ending(
        &next_request(&mut requests),
        &[b"\x89P"],
        Some(Err("the disk filled".to_owned())),
    );
    let error = block_on(pasted).expect_err("a broken file fails the paste");
    assert_eq!(error.to_string(), "the disk filled");
}

#[test]
fn the_requests_end_once_another_copy_replaces_the_source() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, vec![form(0, "text/html")]);
    let mut requests = source.requests();
    assert!(now(requests.next()).is_none(), "no paste yet");
    platform.set(Vec::new());
    assert!(block_on(requests.next()).is_none(), "the stream has ended");
    assert!(
        block_on(source.requests().next()).is_none(),
        "a second call has none"
    );
}

#[test]
fn what_the_program_gave_before_its_end_answers_later_pastes_without_it() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, vec![form(0, "text/html"), form(1, "image/png")]);
    let sources = ProgramSources::default();
    sources.wrote(source.clone());
    let mut requests = source.requests();
    let mut kept = Box::pin(sources.keep_promised());
    assert!(now(kept.as_mut()).is_none());
    for _ in 0..2 {
        let request = now(requests.next())
            .flatten()
            .expect("each form is asked for");
        let bytes = request.form().mime.clone().into_bytes();
        answer(&request, &bytes);
    }
    block_on(kept);
    drop(requests);

    let pasted = (source.producer(form(1, "image/png")))();
    assert_eq!(
        block_on(pasted),
        Some(b"image/png".to_vec()),
        "no request: it is kept"
    );
}

#[test]
fn a_replaced_copy_is_not_asked_for_before_the_end() {
    let platform = PlatformClipboard::new(Some(Arc::new(ListingShell)));
    let source = written_source(&platform, vec![form(0, "text/html")]);
    let sources = ProgramSources::default();
    sources.wrote(source.clone());
    let mut requests = source.requests();
    platform.set(Vec::new());
    assert_eq!(now(sources.keep_promised()), Some(()));
    assert!(block_on(requests.next()).is_none());
}

#[test]
fn a_wait_ends_at_its_limit() {
    let never = std::future::pending::<()>();
    assert_eq!(block_on(within(Duration::from_millis(20), never)), None);
    assert_eq!(
        block_on(within(Duration::from_secs(5), async { 7 })),
        Some(7)
    );
}
