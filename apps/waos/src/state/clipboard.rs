//! The platform's clipboard as the engine's shell has it: Waft's, on the main thread. Each
//! listing, write and read the engine asks for, arriving from `logic::clipboard_calls`, is
//! carried out through it when it is asked, and what is on it is told to the engine as WaOS
//! starts, and each time Waft says another app changed it or a copy expired and was cleared
//! (`ServicesBinding::clipboard`).
//!
//! When WaOS hears of another app's copy is Waft's host's to say, and decides only when the
//! programs are told the clipboard changed, never what a listing or a paste holds. On macOS it
//! looks at the pasteboard's `changeCount` at the moments AppKit leaves an app to look, as
//! Flutter's macOS embedder does: when WaOS is activated and when one of its windows gains the
//! focus, its own writes counted as seen. WaOS does not poll, so the programs are told of a copy
//! made while WaOS stays as it is, as one made while a program's own app has the focus or one
//! from another device through Universal Clipboard, at the next of those moments.
//!
//! Every listing made is kept while the engine holds one of its items, each item reaching the
//! engine by its listing's number and its place in it. Whether an item still reads is Waft's
//! item's to say: it fails with `changed` once anything has replaced what it was listed from.
//!
//! The invariant: one task writes and lists, taking the engine's calls and Waft's word in the
//! order they arrive, so what the engine is told the clipboard changed to is always the latest
//! listing; only reads, which change nothing, run beside it.

use std::collections::HashMap;
use std::future::ready;
use std::sync::Arc;

use futures::channel::mpsc::{self, UnboundedReceiver};
use futures::channel::oneshot;
use futures::stream::{self, StreamExt};
use waft::ui::{ClipboardError as WaftError, ClipboardReadFuture, DataTransferItem, Platform};
use waft::{App, AsyncApp, ListenableObject, Listener};
use waft_services::ServicesBinding;
use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{CopiedItem, CopyOptions, ShellClipboard, WaOS};

use crate::logic::clipboard::{program_error, waft_items, waft_options};
use crate::logic::clipboard_calls::{
    ClipboardCall, ClipboardOnRequest, Listed, ListedPlace, Written,
};

/// Waft's clipboard, served to the engine as its shell's.
pub(crate) struct Clipboard {
    calls: ClipboardOnRequest,
    arriving: UnboundedReceiver<ClipboardCall>,
}

impl Clipboard {
    /// Waft's clipboard on `platform`, to serve to the engine; none where the platform has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<Clipboard> {
        platform.clipboard()?;
        let (calls, arriving) = ClipboardOnRequest::new();
        Some(Clipboard { calls, arriving })
    }

    /// The clipboard as the engine's shell has it: its calls go to the main thread, where
    /// [`serve`](Self::serve) carries them out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellClipboard> {
        Arc::new(self.calls.clone())
    }

    /// From now on, the engine's calls are carried out through Waft's clipboard, and `engine` is
    /// told what is on it: now, and at each change Waft tells.
    pub(crate) fn serve(self, app: &mut App, engine: Arc<WaOS>) {
        let calls = self.arriving.map(Arrival::Call);
        let changes = changes_told(app).map(|()| Arrival::Changed);
        let mut arrivals = stream::select(calls, changes);
        let mut served = Served {
            calls: self.calls,
            listings: Listings::default(),
        };
        app.spawn(async move |cx| {
            engine.set_clipboard(served.list(cx).await);
            while let Some(arrival) = arrivals.next().await {
                served.take(cx, &engine, arrival).await;
            }
        });
    }
}

/// What the one task takes, in the order it arrives.
enum Arrival {
    Call(ClipboardCall),
    /// Waft says another app changed the clipboard, or a copy expired and was cleared.
    Changed,
}

/// Each time Waft tells the clipboard changed, from now on.
fn changes_told(app: &mut App) -> UnboundedReceiver<()> {
    let (told, changes) = mpsc::unbounded();
    let clipboard = ServicesBinding::instance(app).clipboard(app);
    let listener = Listener::new(move |_app| {
        let _ = told.unbounded_send(());
    });
    clipboard.add_listener(app, listener);
    changes
}

/// Waft's clipboard as the task serves it: its listings, and the calls each item's handle
/// makes.
struct Served {
    calls: ClipboardOnRequest,
    listings: Listings,
}

/// Waft's listings of the clipboard that the engine still holds an item of, each by its number,
/// counting from one.
#[derive(Default)]
struct Listings {
    /// The number of the listing made last; none made yet at zero.
    last: u64,
    kept: HashMap<u64, Vec<Box<dyn DataTransferItem>>>,
}

impl Served {
    /// Carries out what arrived: a listing and a write answered once done, a read beside this
    /// task, a listing the engine is done with forgotten, and a change Waft tells told to
    /// `engine`.
    async fn take(&mut self, cx: &mut AsyncApp, engine: &WaOS, arrival: Arrival) {
        match arrival {
            Arrival::Call(ClipboardCall::List { listed }) => {
                let _ = listed.send(self.list(cx).await);
            }
            Arrival::Call(ClipboardCall::Write {
                items,
                options,
                listed,
            }) => {
                let _ = listed.send(self.write(cx, items, options).await);
            }
            Arrival::Call(ClipboardCall::Read {
                item,
                mime,
                content,
            }) => self.read(cx, item, &mime, content),
            Arrival::Call(ClipboardCall::Forget { listing }) => self.listings.forget(listing),
            Arrival::Changed => engine.set_clipboard(self.list(cx).await),
        }
    }

    /// Writes `items` through Waft, and answers what is on the clipboard after: what the engine
    /// tells its programs it changed to.
    async fn write(
        &mut self,
        cx: &mut AsyncApp,
        items: Vec<CopiedItem>,
        options: CopyOptions,
    ) -> Written {
        let written = cx.update(|app| {
            let platform = app.platform();
            let clipboard = platform.clipboard()?;
            Some(clipboard.write(waft_items(items), waft_options(options)))
        });
        let Some(written) = written else {
            return Err(ClipboardError::Unsupported);
        };
        written.await.map_err(program_error)?;
        Ok(self.list(cx).await)
    }

    /// Lists what is on the clipboard now, keeps the listing, and answers its items as the
    /// engine reaches them.
    async fn list(&mut self, cx: &mut AsyncApp) -> Listed {
        let listed = cx.update(|app| {
            app.platform()
                .clipboard()
                .map(|clipboard| clipboard.items())
        });
        let items = match listed {
            Some(listed) => listed.await,
            None => Vec::new(),
        };
        let types = items.iter().map(|item| item.types().to_vec()).collect();
        let listing = self.listings.keep(items);
        self.calls.listed(listing, types)
    }

    /// Reads the item at `place` in `mime`, in a task beside this one, answering into `content`.
    fn read(
        &self,
        cx: &mut AsyncApp,
        place: ListedPlace,
        mime: &str,
        content: oneshot::Sender<Result<Vec<u8>, ClipboardError>>,
    ) {
        let read = self.listings.read(place, mime);
        cx.update(|app| {
            app.spawn(async move |_cx| {
                let _ = content.send(read.await.map_err(program_error));
            })
        });
    }
}

impl Listings {
    /// Keeps `items`, a listing just made; answers its number.
    fn keep(&mut self, items: Vec<Box<dyn DataTransferItem>>) -> u64 {
        self.last += 1;
        self.kept.insert(self.last, items);
        self.last
    }

    fn forget(&mut self, listing: u64) {
        self.kept.remove(&listing);
    }

    /// The item at `place` in `mime`, as Waft's item reads it now.
    fn read(&self, place: ListedPlace, mime: &str) -> ClipboardReadFuture {
        let item = self.kept.get(&place.listing);
        match item.and_then(|items| items.get(place.index)) {
            Some(item) => item.read(mime),
            None => Box::pin(ready(Err(WaftError::NotFound))),
        }
    }
}
