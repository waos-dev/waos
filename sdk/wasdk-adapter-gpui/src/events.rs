//! The program's streams as one: everything the event loop turns on.
//!
//! Each view's events arrive in order on the view's one stream. Beside them: the changes among
//! the program's views, the screens and what the user set, the platform's requests to end the
//! program, and the loop's own notices, which reach it from outside a
//! stream: queued work, and the app quitting.
//!
//! The invariant: every stream is opened before the value it changes is first read, so no change
//! falls between the two.

use futures::channel::mpsc;
use futures::stream::{LocalBoxStream, SelectAll, StreamExt};
use wasdk::StreamReader;
use wasdk::lifecycle::{self, ExitRequest};
use wasdk::screens;
use wasdk::system::{self, SystemChange};
use wasdk::views::{self, View, ViewChange, ViewEvent};

/// One event from any of the program's streams, or a notice to the loop.
pub(crate) enum Event {
    View(u64, ViewEvent),
    ViewChange(ViewChange),
    ScreensChanged,
    SystemChange(SystemChange),
    ExitRequest(ExitRequest),
    Notice(Notice),
}

/// What reaches the loop from outside the platform's streams.
pub(crate) enum Notice {
    /// Work was queued on the dispatcher.
    Wake,
    /// Work GPUI wants run apart, for audio; with one thread, it runs next.
    Realtime(Box<dyn FnOnce() + Send>),
    /// The app quit.
    Quit,
}

/// Every stream the loop reads, as one.
pub(crate) struct Events {
    streams: SelectAll<LocalBoxStream<'static, Event>>,
}

impl Events {
    /// The program's streams beside the views', opened now; and the loop's `notices`.
    pub(crate) fn open(notices: mpsc::UnboundedReceiver<Notice>) -> Events {
        let mut streams = SelectAll::new();
        streams.push(notices.map(Event::Notice).boxed_local());
        streams.push(merged(views::view_changes(), Event::ViewChange));
        streams.push(merged(screens::screen_changes(), |_| Event::ScreensChanged));
        streams.push(merged(system::changes(), Event::SystemChange));
        streams.push(merged(lifecycle::exit_requests(), Event::ExitRequest));
        Events { streams }
    }

    /// Reads `view`'s events, in the order they happened, until the view is gone.
    pub(crate) fn add_view(&mut self, view: &View) {
        let id = view.id();
        if let Some(events) = view.events() {
            self.streams
                .push(merged(events, move |event| Event::View(id, event)));
        }
    }

    pub(crate) async fn next(&mut self) -> Option<Event> {
        self.streams.next().await
    }
}

fn merged<T: 'static>(
    stream: StreamReader<T>,
    event: impl FnMut(T) -> Event + 'static,
) -> LocalBoxStream<'static, Event> {
    stream.into_stream().map(event).boxed_local()
}
