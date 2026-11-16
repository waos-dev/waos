//! The platform's clipboard as the engine's shell has it: Waft's, on the main thread. Each
//! program's write and read, arriving from `logic::clipboard_calls`, is carried out through it, and
//! what is on it is listed into the engine as WaOS starts, and each time Waft says another app
//! changed it or a copy expired and was cleared (`ServicesBinding::clipboard`).
//!
//! When WaOS hears of another app's copy is Waft's host's to say. On macOS it looks at the
//! pasteboard's `changeCount` at the moments AppKit leaves an app to look, as Flutter's macOS
//! embedder does: when WaOS is activated and when one of its windows gains the focus, its own
//! writes counted as seen. WaOS does not poll, so a copy that reaches the pasteboard while WaOS
//! stays active, as one from another device through Universal Clipboard, reaches the programs at
//! the next of those moments.
//!
//! Only Waft's latest listing is kept, each of its items reaching the engine by the listing's
//! number and its place in it: a read of an item of an earlier listing fails with `changed`, as
//! the clipboard has changed since.
//!
//! The invariant: one task writes and lists, taking the programs' calls and Waft's word in the
//! order they arrive, so the listing kept is always the latest, and so is the one the engine is
//! given; only reads, which change nothing, run beside it.

use std::future::ready;
use std::sync::Arc;

use futures::channel::mpsc::{self, UnboundedReceiver};
use futures::channel::oneshot;
use futures::stream::{self, StreamExt};
use waft::ui::{ClipboardError as WaftError, ClipboardReadFuture, DataTransferItem, Platform};
use waft::{App, AsyncApp, ListenableObject, Listener};
use waft_services::ServicesBinding;
use waos::bindings::wasdk::platform::clipboard::ClipboardError;
use waos::{CopiedItem, CopyOptions, ShellClipboard, ShellClipboardItem, WaOS};

use crate::logic::clipboard::{program_error, waft_items, waft_options};
use crate::logic::clipboard_calls::{ClipboardCall, ClipboardOnRequest, Listed, ListedPlace};

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

    /// From now on, the programs' calls are carried out through Waft's clipboard, and what is on
    /// it is listed into `engine`: now, and at each change Waft tells.
    pub(crate) fn serve(self, app: &mut App, engine: Arc<WaOS>) {
        let calls = self.arriving.map(Arrival::Call);
        let changes = changes_told(app).map(|()| Arrival::Changed);
        let mut arrivals = stream::select(calls, changes);
        let mut served = Served {
            calls: self.calls,
            listing: Listing::default(),
        };
        app.spawn(async move |cx| {
            engine.set_clipboard(served.relist(cx).await);
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

/// Waft's clipboard as the task serves it: its latest listing, and the calls each item's handle
/// makes.
struct Served {
    calls: ClipboardOnRequest,
    listing: Listing,
}

/// Waft's latest listing of the clipboard, by its number, counting from one; none before the
/// first.
#[derive(Default)]
struct Listing {
    number: u64,
    items: Vec<Box<dyn DataTransferItem>>,
}

impl Served {
    /// Carries out what arrived: a write answered once it is done, a read beside this task, and
    /// a change Waft tells listed into `engine`.
    async fn take(&mut self, cx: &mut AsyncApp, engine: &WaOS, arrival: Arrival) {
        match arrival {
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
            Arrival::Changed => engine.set_clipboard(self.relist(cx).await),
        }
    }

    /// Writes `items` through Waft, and answers what is on the clipboard after, as the engine's
    /// listing from then on.
    async fn write(
        &mut self,
        cx: &mut AsyncApp,
        items: Vec<CopiedItem>,
        options: CopyOptions,
    ) -> Listed {
        let written = cx.update(|app| {
            let platform = app.platform();
            let clipboard = platform.clipboard()?;
            Some(clipboard.write(waft_items(items), waft_options(options)))
        });
        let Some(written) = written else {
            return Err(ClipboardError::Unsupported);
        };
        written.await.map_err(program_error)?;
        Ok(self.relist(cx).await)
    }

    /// Lists what is on the clipboard now, keeps it as the latest listing, and answers its items
    /// as the engine reaches them.
    async fn relist(&mut self, cx: &mut AsyncApp) -> Vec<Arc<dyn ShellClipboardItem>> {
        let listed = cx.update(|app| {
            app.platform()
                .clipboard()
                .map(|clipboard| clipboard.items())
        });
        let items = match listed {
            Some(listed) => listed.await,
            None => Vec::new(),
        };
        self.listing = Listing {
            number: self.listing.number + 1,
            items,
        };
        let number = self.listing.number;
        let handles = self.listing.items.iter().enumerate().map(|(index, item)| {
            let place = ListedPlace {
                listing: number,
                index,
            };
            self.calls.listed(place, item.types().to_vec())
        });
        handles.collect()
    }

    /// Reads the item at `place` in `mime`, in a task beside this one, answering into `content`.
    fn read(
        &self,
        cx: &mut AsyncApp,
        place: ListedPlace,
        mime: &str,
        content: oneshot::Sender<Result<Vec<u8>, ClipboardError>>,
    ) {
        let read = self.listing.read(place, mime);
        cx.update(|app| {
            app.spawn(async move |_cx| {
                let _ = content.send(read.await.map_err(program_error));
            })
        });
    }
}

impl Listing {
    /// The item at `place` in `mime`; `changed` for an item of an earlier listing.
    fn read(&self, place: ListedPlace, mime: &str) -> ClipboardReadFuture {
        if place.listing != self.number {
            return Box::pin(ready(Err(WaftError::Changed)));
        }
        match self.items.get(place.index) {
            Some(item) => item.read(mime),
            None => Box::pin(ready(Err(WaftError::NotFound))),
        }
    }
}
