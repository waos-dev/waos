//! Where the programs' clipboard calls go: each to the main thread, where Waft's clipboard is
//! (`state::clipboard`). The engine calls from a program's thread and is answered later, through
//! the future it was given: a listing with what is on the clipboard, a write with what is on it
//! after, a read with the item's content.
//!
//! An item the engine holds is a place in one of the main thread's listings. The items of a
//! listing share its hold on it ([`KeptListing`]): once the engine has dropped the last of them,
//! the main thread is told to forget the listing.
//!
//! The invariant: every call taken reaches the main thread or is answered at once, so no program
//! waits for an answer that cannot come.

use std::sync::Arc;

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{
    ClipboardItemsFuture, ClipboardReadFuture, ClipboardWriteFuture, CopiedItem, CopyOptions,
    ShellClipboard, ShellClipboardItem,
};

/// What is on the clipboard, as the engine reaches it.
pub(crate) type Listed = Vec<Arc<dyn ShellClipboardItem>>;

/// What is on the clipboard after a write, or why it was not written.
pub(crate) type Written = Result<Listed, ClipboardError>;

/// One clipboard call of the engine's, on its way to the main thread.
pub(crate) enum ClipboardCall {
    /// What is on the clipboard now.
    List { listed: oneshot::Sender<Listed> },
    /// A copy, answered with what is on the clipboard after it.
    Write {
        items: Vec<CopiedItem>,
        options: CopyOptions,
        listed: oneshot::Sender<Written>,
    },
    /// A read of one item in one of its types.
    Read {
        item: ListedPlace,
        mime: String,
        content: oneshot::Sender<Result<Vec<u8>, ClipboardError>>,
    },
    /// The engine holds no item of the listing of this number any more.
    Forget { listing: u64 },
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

    /// The items of the main thread's listing `listing`, each offered in its `types`, as the
    /// engine reaches them: the listing is kept until the engine has dropped them all.
    pub(crate) fn listed(&self, listing: u64, types: Vec<Vec<String>>) -> Listed {
        let kept = Arc::new(KeptListing {
            number: listing,
            calls: self.calls.clone(),
        });
        let items = types.into_iter().enumerate().map(|(index, types)| {
            Arc::new(ListedOnRequest {
                index,
                types,
                listing: Arc::clone(&kept),
            }) as Arc<dyn ShellClipboardItem>
        });
        items.collect()
    }
}

impl ShellClipboard for ClipboardOnRequest {
    /// Asked of the main thread as this is called; nothing once WaOS is quitting.
    fn items(&self) -> ClipboardItemsFuture {
        let (listed, answered) = oneshot::channel();
        if self
            .calls
            .unbounded_send(ClipboardCall::List { listed })
            .is_err()
        {
            tracing::info!("WaOS is quitting: the clipboard is not listed");
        }
        Box::pin(answered.map(Result::unwrap_or_default))
    }

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

/// The engine's hold on one of the main thread's listings, shared by the listing's items: its
/// drop tells the main thread to forget the listing.
struct KeptListing {
    number: u64,
    calls: UnboundedSender<ClipboardCall>,
}

impl Drop for KeptListing {
    fn drop(&mut self) {
        let listing = self.number;
        // Refused only once WaOS is quitting, when the listings go with the main thread's task.
        let _ = self.calls.unbounded_send(ClipboardCall::Forget { listing });
    }
}

/// One item as the main thread listed it: its read goes there.
struct ListedOnRequest {
    index: usize,
    types: Vec<String>,
    listing: Arc<KeptListing>,
}

impl ShellClipboardItem for ListedOnRequest {
    fn types(&self) -> &[String] {
        &self.types
    }

    fn read(&self, mime: &str) -> ClipboardReadFuture {
        let (content, answered) = oneshot::channel();
        let call = ClipboardCall::Read {
            item: ListedPlace {
                listing: self.listing.number,
                index: self.index,
            },
            mime: mime.to_owned(),
            content,
        };
        if self.listing.calls.unbounded_send(call).is_err() {
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

    fn offered_as_text() -> Vec<Vec<String>> {
        vec![vec!["text/plain".to_owned()]]
    }

    #[test]
    fn a_listing_is_asked_of_the_main_thread_as_it_is_called_for() {
        let (clipboard, mut calls) = ClipboardOnRequest::new();
        let items = clipboard.items();
        let Some(ClipboardCall::List { listed }) = block_on(calls.next()) else {
            panic!("the call arrived before its answer was awaited");
        };
        let _ = listed.send(clipboard.listed(4, offered_as_text()));
        assert_eq!(block_on(items)[0].types(), ["text/plain"]);
    }

    #[test]
    fn a_write_reaches_the_main_thread_and_is_answered_with_what_it_lists() {
        let (clipboard, mut calls) = ClipboardOnRequest::new();
        let written = clipboard.write(Vec::new(), CopyOptions::default());
        let Some(ClipboardCall::Write { listed, .. }) = block_on(calls.next()) else {
            panic!("a write arrives");
        };
        let _ = listed.send(Ok(clipboard.listed(2, offered_as_text())));
        let items = block_on(written).expect("written");
        assert_eq!(items[0].types(), ["text/plain"]);
    }

    #[test]
    fn a_read_names_its_item_and_type_on_the_main_thread() {
        let (clipboard, mut calls) = ClipboardOnRequest::new();
        let types = vec![Vec::new(); 4];
        let read = clipboard.listed(1, types)[3].read("text/html");
        let Some(ClipboardCall::Read {
            item,
            mime,
            content,
        }) = block_on(calls.next())
        else {
            panic!("a read arrives");
        };
        let place = ListedPlace {
            listing: 1,
            index: 3,
        };
        assert_eq!((item, mime.as_str()), (place, "text/html"));
        let _ = content.send(Ok(b"<b>hi</b>".to_vec()));
        assert_eq!(block_on(read).expect("read"), b"<b>hi</b>");
    }

    #[test]
    fn a_listing_is_forgotten_once_the_last_of_its_items_is_dropped() {
        let (clipboard, mut calls) = ClipboardOnRequest::new();
        let mut items = clipboard.listed(7, vec![Vec::new(); 2]);
        items.pop();
        assert!(calls.try_recv().is_err(), "one item of it is still held");
        items.pop();
        let forgotten = block_on(calls.next());
        assert!(matches!(
            forgotten,
            Some(ClipboardCall::Forget { listing: 7 })
        ));
    }

    #[test]
    fn a_call_the_main_thread_drops_or_never_takes_fails() {
        let (clipboard, calls) = ClipboardOnRequest::new();
        let items = clipboard.listed(1, offered_as_text());
        let read = items[0].read("text/plain");
        drop(calls);
        let written = clipboard.write(Vec::new(), CopyOptions::default());
        assert!(matches!(block_on(read), Err(ClipboardError::Failed(_))));
        assert!(matches!(block_on(written), Err(ClipboardError::Failed(_))));
        assert!(block_on(clipboard.items()).is_empty(), "nothing listed");
    }
}
