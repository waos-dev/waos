//! The program's streams as one: everything the loop turns on.
//!
//! Each view's events arrive in the order they happened on the view's one stream. Beside them are
//! merged the changes among the program's views, the screens and what the user set, the program's
//! lifecycle states, the platform's requests to end the program, the installed fonts' changes, the
//! clipboard's changes where the platform has one, the items chosen in the menu bar where it has
//! one, what the user opens with the program, what the user does to its notifications where the
//! platform has them, the streams the platform opens as the app goes ([`StreamOpener`]): each tray
//! icon's events and its attached menu's choices; and the loop's own notices: what reaches the loop
//! from outside a stream, a ready task asking to be woken and the app quitting. The notices never
//! end while the platform holds their sender, so the loop's end is a quit, never an exhausted
//! stream.
//!
//! The invariants: every stream is opened before the value it changes is first read, so no change
//! falls between the two; and a stream's next item is read only once the loop asks for the next
//! event, after it has pushed the one before into the client.

use futures::channel::mpsc;
use futures::future::{self, Either};
use futures::stream::{LocalBoxStream, SelectAll, StreamExt};
use waft_embedder::{MenuId, TrayIconEvent, TrayIconId, View, ViewId};
use wasdk::StreamReader;
use wasdk::lifecycle::{self, ExitRequest, LifecycleState};
use wasdk::screens::{self, ScreenChange};
use wasdk::system::{self, SystemChange};
use wasdk::views::{self, ViewChange, ViewEvent};
use wasdk::{clipboard, menus, notifications};

use crate::views::WasdkView;

/// One event from any of the program's streams, or a notice to the loop.
pub(crate) enum Event {
    /// What happened to one view.
    View(ViewId, ViewEvent),
    /// A view arrived or went, or a request got none.
    ViewChange(ViewChange),
    /// A screen was added, removed or changed.
    ScreenChange(ScreenChange),
    /// The user changed light or dark, or the languages.
    SystemChange(SystemChange),
    /// The program entered another lifecycle state.
    LifecycleState(LifecycleState),
    /// The platform asks whether the program may end.
    ExitRequest(ExitRequest),
    /// The installed fonts changed.
    FontsChanged,
    /// What is on the clipboard changed: another program or app copied, or this one did.
    ClipboardChanged,
    /// The user chose the item of the menu bar with this id.
    MenuSelected(MenuId),
    /// The user opened files or links with the program.
    OpenRequested(lifecycle::OpenRequest),
    /// The user did something to one of the program's notifications.
    NotificationActivated(notifications::Activation),
    /// The user did something to the app's tray icon with this id, or chose in its attached menu.
    TrayIcon(TrayIconId, TrayIconEvent),
    Notice(Notice),
}

/// What reaches the loop from outside the platform's streams.
pub(crate) enum Notice {
    /// A ready task asked for the framework to be woken.
    WakeNow,
    /// The app quit.
    Quit,
}

/// Every stream the loop reads, as one.
pub(crate) struct Events {
    streams: SelectAll<LocalBoxStream<'static, Event>>,
    /// The streams the platform opened since, to read from now on.
    opened: mpsc::UnboundedReceiver<LocalBoxStream<'static, Event>>,
    opener: StreamOpener,
}

/// Where the platform hands the loop a stream it opened, which the loop reads from then on.
#[derive(Clone)]
pub(crate) struct StreamOpener(mpsc::UnboundedSender<LocalBoxStream<'static, Event>>);

impl StreamOpener {
    /// The loop reads `stream`'s items, each as `event` makes it, from now on.
    pub(crate) fn open<T: 'static>(
        &self,
        stream: StreamReader<T>,
        event: impl FnMut(T) -> Event + 'static,
    ) {
        // The loop holds the receiver for as long as it turns; once it has stopped, nobody reads.
        let _ = self.0.unbounded_send(merged(stream, event));
    }
}

impl Events {
    /// The program's streams beside the views', opened now, before the values they change are
    /// read; and the loop's `notices`. Views are added as they arrive, and the streams the
    /// platform opens through [`opener`](Self::opener).
    pub(crate) fn open(notices: mpsc::UnboundedReceiver<Notice>) -> Events {
        let mut streams = SelectAll::new();
        streams.push(notices.map(Event::Notice).boxed_local());
        streams.push(merged(views::view_changes(), Event::ViewChange));
        streams.push(merged(screens::screen_changes(), Event::ScreenChange));
        streams.push(merged(system::changes(), Event::SystemChange));
        streams.push(merged(lifecycle::state_changes(), Event::LifecycleState));
        streams.push(merged(lifecycle::exit_requests(), Event::ExitRequest));
        streams.push(merged(wasdk::fonts::changes(), |_count| {
            Event::FontsChanged
        }));
        if let Some(clipboard) = clipboard::get_clipboard() {
            streams.push(merged(clipboard.changes(), |_items| {
                Event::ClipboardChanged
            }));
        }
        if let Some(menu_bar) = menus::get_menu_bar() {
            streams.push(merged(menu_bar.chosen(), Event::MenuSelected));
        }
        streams.push(merged(lifecycle::open_requests(), Event::OpenRequested));
        if let Some(notifications) = notifications::get_notifications() {
            let activations = notifications.activations();
            streams.push(merged(activations, Event::NotificationActivated));
        }
        let (opener, opened) = mpsc::unbounded();
        Events {
            streams,
            opened,
            opener: StreamOpener(opener),
        }
    }

    /// Where the platform hands the loop the streams it opens.
    pub(crate) fn opener(&self) -> StreamOpener {
        self.opener.clone()
    }

    /// Reads `view`'s events, in the order they happened, until the view is gone.
    pub(crate) fn add_view(&mut self, view: &WasdkView) {
        let id = view.id();
        if let Some(events) = view.platform_view().events() {
            self.streams
                .push(merged(events, move |event| Event::View(id, event)));
        }
    }

    /// The next event from any stream, reading from now on each stream opened meanwhile.
    pub(crate) async fn next(&mut self) -> Option<Event> {
        loop {
            let Events {
                streams, opened, ..
            } = self;
            match future::select(streams.next(), opened.next()).await {
                Either::Left((event, _)) => return event,
                Either::Right((Some(stream), _)) => self.streams.push(stream),
                // Never while this holds the opener's sender: only the streams are left.
                Either::Right((None, _)) => return self.streams.next().await,
            }
        }
    }
}

/// `stream`'s items as the loop's events.
fn merged<T: 'static>(
    stream: StreamReader<T>,
    event: impl FnMut(T) -> Event + 'static,
) -> LocalBoxStream<'static, Event> {
    stream.into_stream().map(event).boxed_local()
}
