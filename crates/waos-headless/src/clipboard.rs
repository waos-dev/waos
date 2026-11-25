//! The headless shell's clipboard: a clipboard of its own, held in memory. What a program
//! copies is there for that program and every other one of this WaOS to paste, and nothing of
//! it reaches the machine's own clipboard, which a WaOS with no screen has no business
//! changing: a test's copy would replace what its user copied.
//!
//! The invariant: an item is the clipboard as it was listed. Its read fails with `changed` once
//! another copy has replaced the one it was listed from, so a read never lands on another copy.

use std::future::ready;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{
    ClipboardReadFuture, ClipboardWriteFuture, Content, ContentFuture, CopiedItem, CopyOptions,
    ShellClipboard, ShellClipboardItem,
};

/// The type a file is listed and read in: its `file:` URL.
const URI_LIST: &str = "text/uri-list";

/// The clipboard: how many copies it has taken, the last of which is the one on it.
#[derive(Default)]
pub struct HeadlessClipboard {
    copies: Arc<AtomicU64>,
}

impl ShellClipboard for HeadlessClipboard {
    /// Takes the copy in place of the one before, and lists an item for each written, in the
    /// types written, a file's as `text/uri-list`. How the copy is to be treated is not kept:
    /// this clipboard has no history, no other devices, and ends with its WaOS.
    fn write(&self, items: Vec<CopiedItem>, _options: CopyOptions) -> ClipboardWriteFuture {
        let copy = self.copies.fetch_add(1, Ordering::SeqCst) + 1;
        let listed = items
            .into_iter()
            .map(|item| HeldItem::of(item, copy, &self.copies))
            .collect();
        Box::pin(ready(Ok(listed)))
    }
}

/// One item of a copy, as the clipboard holds and lists it.
struct HeldItem {
    types: Vec<String>,
    forms: Vec<(String, Form)>,
    /// Which copy the item is of, and the count that says whether it is still the one on the
    /// clipboard.
    copy: u64,
    copies: Arc<AtomicU64>,
}

/// One form of an item, by how its bytes are had.
enum Form {
    Given(Vec<u8>),
    Promised(Arc<dyn Fn() -> ContentFuture + Send + Sync>),
    File(PathBuf),
    /// A file that exists only once pasted somewhere, which nothing here chooses.
    PromisedFile,
}

impl HeldItem {
    fn of(item: CopiedItem, copy: u64, copies: &Arc<AtomicU64>) -> Arc<dyn ShellClipboardItem> {
        let forms: Vec<(String, Form)> = item
            .representations
            .into_iter()
            .map(|(mime, content)| match content {
                Content::Data(bytes) => (mime, Form::Given(bytes)),
                Content::Promise(produce) => (mime, Form::Promised(produce)),
                Content::File(path) => (URI_LIST.to_owned(), Form::File(path)),
                Content::FilePromise { .. } => (URI_LIST.to_owned(), Form::PromisedFile),
            })
            .collect();
        let mut types: Vec<String> = Vec::new();
        for (mime, _) in &forms {
            if !types.contains(mime) {
                types.push(mime.clone());
            }
        }
        Arc::new(HeldItem {
            types,
            forms,
            copy,
            copies: Arc::clone(copies),
        })
    }

    fn is_on_the_clipboard(&self) -> bool {
        self.copies.load(Ordering::SeqCst) == self.copy
    }
}

impl ShellClipboardItem for HeldItem {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        if !self.is_on_the_clipboard() {
            return Box::pin(ready(Err(ClipboardError::Changed)));
        }
        let Some((_, form)) = self.forms.iter().find(|(offered, _)| offered == mime) else {
            return Box::pin(ready(Err(ClipboardError::NotFound)));
        };
        match form {
            Form::Given(bytes) => Box::pin(ready(Ok(bytes.clone()))),
            Form::Promised(produce) => {
                let produced = produce();
                Box::pin(async move { produced.await.ok_or_else(not_produced) })
            }
            Form::File(path) => Box::pin(ready(file_url(path))),
            Form::PromisedFile => Box::pin(ready(Err(ClipboardError::Unsupported))),
        }
    }
}

fn not_produced() -> ClipboardError {
    ClipboardError::Failed("the program that copied it did not produce it in time".to_owned())
}

/// `path` as a `text/uri-list` of one: its `file:` URL.
fn file_url(path: &std::path::Path) -> Result<Vec<u8>, ClipboardError> {
    url::Url::from_file_path(path)
        .map(|url| url.to_string().into_bytes())
        .map_err(|()| ClipboardError::Failed(format!("{} is no absolute path", path.display())))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use super::*;

    /// What a future that never waits answers.
    fn now<T>(future: impl Future<Output = T>) -> T {
        match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(answer) => answer,
            Poll::Pending => panic!("the clipboard answers at once"),
        }
    }

    fn text(text: &str) -> CopiedItem {
        CopiedItem {
            representations: vec![(
                "text/plain".to_owned(),
                Content::Data(text.as_bytes().to_vec()),
            )],
        }
    }

    fn copied(
        clipboard: &HeadlessClipboard,
        items: Vec<CopiedItem>,
    ) -> Vec<Arc<dyn ShellClipboardItem>> {
        match now(clipboard.write(items, CopyOptions::default())) {
            Ok(listed) => listed,
            Err(error) => panic!("the clipboard takes every copy: {error:?}"),
        }
    }

    #[test]
    fn what_a_program_copies_is_listed_and_read_back() {
        let clipboard = HeadlessClipboard::default();
        let listed = copied(&clipboard, vec![text("milk")]);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].types(), ["text/plain"]);
        assert_eq!(
            now(listed[0].read("text/plain")).ok(),
            Some(b"milk".to_vec())
        );
        assert!(matches!(
            now(listed[0].read("image/png")),
            Err(ClipboardError::NotFound)
        ));
    }

    #[test]
    fn an_item_of_a_copy_since_replaced_is_no_longer_read() {
        let clipboard = HeadlessClipboard::default();
        let first = copied(&clipboard, vec![text("milk")]);
        let second = copied(&clipboard, vec![text("eggs")]);
        assert!(matches!(
            now(first[0].read("text/plain")),
            Err(ClipboardError::Changed)
        ));
        assert_eq!(
            now(second[0].read("text/plain")).ok(),
            Some(b"eggs".to_vec())
        );
    }

    #[test]
    fn a_copy_of_nothing_empties_the_clipboard() {
        let clipboard = HeadlessClipboard::default();
        let filled = copied(&clipboard, vec![text("milk")]);
        assert!(copied(&clipboard, Vec::new()).is_empty());
        assert!(matches!(
            now(filled[0].read("text/plain")),
            Err(ClipboardError::Changed)
        ));
    }

    #[test]
    fn a_promised_form_is_produced_as_it_is_read() {
        let clipboard = HeadlessClipboard::default();
        let produce: Arc<dyn Fn() -> ContentFuture + Send + Sync> =
            Arc::new(|| Box::pin(ready(Some(b"made on paste".to_vec()))));
        let item = CopiedItem {
            representations: vec![("text/plain".to_owned(), Content::Promise(produce))],
        };
        let listed = copied(&clipboard, vec![item]);
        assert_eq!(
            now(listed[0].read("text/plain")).ok(),
            Some(b"made on paste".to_vec())
        );
    }

    #[test]
    fn a_file_is_listed_and_read_as_its_url() {
        let clipboard = HeadlessClipboard::default();
        let item = CopiedItem {
            representations: vec![(
                "application/pdf".to_owned(),
                Content::File(PathBuf::from("/tmp/a report.pdf")),
            )],
        };
        let listed = copied(&clipboard, vec![item]);
        assert_eq!(listed[0].types(), [URI_LIST]);
        assert_eq!(
            now(listed[0].read(URI_LIST)).ok(),
            Some(b"file:///tmp/a%20report.pdf".to_vec())
        );
    }
}
