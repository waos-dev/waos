//! Where the programs' clipboard calls go: each to the main thread, where Waft's clipboard is
//! (`state::clipboard`). The engine calls from a program's thread and is answered later, through
//! the future it was given: a write with what is on the clipboard after it, a read with the
//! item's content.
//!
//! The invariant: every call taken reaches the main thread or is answered at once, so no program
//! waits for an answer that cannot come.

use std::sync::Arc;

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{
    ClipboardReadFuture, ClipboardWriteFuture, CopiedItem, CopyOptions, ShellClipboard,
    ShellClipboardItem,
};

/// What is on the clipboard after a write, or why it was not written.
pub(crate) type Listed = Result<Vec<Arc<dyn ShellClipboardItem>>, ClipboardError>;

/// One program's clipboard call, on its way to the main thread.
pub(crate) enum ClipboardCall {
    /// A copy, answered with what is on the clipboard after it.
    Write {
        items: Vec<CopiedItem>,
        options: CopyOptions,
        listed: oneshot::Sender<Listed>,
    },
    /// A read of one item in one of its types.
    Read {
        item: ListedPlace,
        mime: String,
        content: oneshot::Sender<Result<Vec<u8>, ClipboardError>>,
    },
}

/// Where an item is in the main thread's listings of the clipboard: which listing, by its
/// number, and its place in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ListedPlace {
    pub(crate) listing: u64,
    pub(crate) index: usize,
}

/// The app's clipboard, as the engine reaches it: every call goes to the main thread. Every clone
/// sends to the same.
#[derive(Clone)]
pub(crate) struct ClipboardOnRequest {
    calls: UnboundedSender<ClipboardCall>,
}

impl ClipboardOnRequest {
    /// The clipboard, and where its calls arrive on the main thread.
    pub(crate) fn new() -> (ClipboardOnRequest, UnboundedReceiver<ClipboardCall>) {
        let (calls, arriving) = mpsc::unbounded();
        (ClipboardOnRequest { calls }, arriving)
    }

    /// The item at `place`, offered in `types`, as the engine reaches it.
    pub(crate) fn listed(
        &self,
        place: ListedPlace,
        types: Vec<String>,
    ) -> Arc<dyn ShellClipboardItem> {
        Arc::new(ListedOnRequest {
            place,
            types,
            calls: self.calls.clone(),
        })
    }
}

impl ShellClipboard for ClipboardOnRequest {
    fn write(&self, items: Vec<CopiedItem>, options: CopyOptions) -> ClipboardWriteFuture {
        let (listed, answered) = oneshot::channel();
        let call = ClipboardCall::Write {
            items,
            options,
            listed,
        };
        if self.calls.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: the clipboard is not written");
        }
        Box::pin(answered.map(|listed| listed.unwrap_or_else(|_| Err(quitting()))))
    }
}

/// One item as the main thread listed it: its read goes there.
struct ListedOnRequest {
    place: ListedPlace,
    types: Vec<String>,
    calls: UnboundedSender<ClipboardCall>,
}

impl ShellClipboardItem for ListedOnRequest {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let (content, answered) = oneshot::channel();
        let call = ClipboardCall::Read {
            item: self.place,
            mime: mime.to_owned(),
            content,
        };
        if self.calls.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: the clipboard is not read");
        }
        Box::pin(answered.map(|content| content.unwrap_or_else(|_| Err(quitting()))))
    }
}

/// Why a call dropped unanswered failed, here or on the main thread.
fn quitting() -> ClipboardError {
    ClipboardError::Failed("WaOS is quitting".to_owned())
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn a_write_reaches_the_main_thread_and_is_answered_with_what_it_lists() {
        let (clipboard, mut calls) = ClipboardOnRequest::new();
        let written = clipboard.write(Vec::new(), CopyOptions::default());
        let Some(ClipboardCall::Write { listed, .. }) = block_on(calls.next()) else {
            panic!("a write arrives");
        };
        let place = ListedPlace {
            listing: 2,
            index: 0,
        };
        let item = clipboard.listed(place, vec!["text/plain".to_owned()]);
        let _ = listed.send(Ok(vec![item]));
        let items = block_on(written).expect("written");
        assert_eq!(items[0].types(), ["text/plain"]);
    }

    #[test]
    fn a_read_names_its_item_and_type_on_the_main_thread() {
        let (clipboard, mut calls) = ClipboardOnRequest::new();
        let place = ListedPlace {
            listing: 1,
            index: 3,
        };
        let read = clipboard.listed(place, Vec::new()).read("text/html");
        let Some(ClipboardCall::Read {
            item,
            mime,
            content,
        }) = block_on(calls.next())
        else {
            panic!("a read arrives");
        };
        assert_eq!((item, mime.as_str()), (place, "text/html"));
        let _ = content.send(Ok(b"<b>hi</b>".to_vec()));
        assert_eq!(block_on(read).expect("read"), b"<b>hi</b>");
    }

    #[test]
    fn a_call_the_main_thread_drops_or_never_takes_fails() {
        let (clipboard, calls) = ClipboardOnRequest::new();
        let read = clipboard
            .listed(
                ListedPlace {
                    listing: 1,
                    index: 0,
                },
                Vec::new(),
            )
            .read("text/plain");
        drop(calls);
        let written = clipboard.write(Vec::new(), CopyOptions::default());
        assert!(matches!(block_on(read), Err(ClipboardError::Failed(_))));
        assert!(matches!(block_on(written), Err(ClipboardError::Failed(_))));
    }
}
