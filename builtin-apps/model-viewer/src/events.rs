//! Everything the app waits for, on one queue: its view's events, what the user opens with it,
//! the file a dialog answers with, and the system's light or dark.
//!
//! Each source is a stream of its own on the platform's side; a task per source forwards it here,
//! so the app's loop reads one queue and each stream keeps its own order.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use wasdk::system::{Brightness, SystemChange};
use wasdk::views::ViewEvent;
use wasdk::{lifecycle, system};
use wasdk_wgpu::SharedView;

pub enum AppEvent {
    View(ViewEvent),
    /// The view's events ended: its window was closed, or the platform took it away.
    ViewGone,
    /// A file to show, at its path inside the program: one opened with the app, or picked.
    FileChosen(String),
    Brightness(Brightness),
}

pub type Sender = UnboundedSender<AppEvent>;

/// The queue, with `view`'s events, the files opened with the app and the system's changes
/// already flowing into it.
pub fn queue(view: &SharedView) -> Result<(Sender, UnboundedReceiver<AppEvent>), String> {
    let (sender, receiver) = unbounded();
    forward_view_events(view, sender.clone())?;
    forward_open_requests(sender.clone());
    forward_system_changes(sender.clone());
    Ok((sender, receiver))
}

fn forward_view_events(view: &SharedView, sender: Sender) -> Result<(), String> {
    let mut events = view
        .events()
        .ok_or("the view's events were taken already")?;
    wasdk::spawn(async move {
        while let Some(event) = events.next().await {
            if sender.unbounded_send(AppEvent::View(event)).is_err() {
                return;
            }
        }
        let _ = sender.unbounded_send(AppEvent::ViewGone);
    });
    Ok(())
}

fn forward_open_requests(sender: Sender) {
    let mut requests = lifecycle::open_requests();
    wasdk::spawn(async move {
        while let Some(request) = requests.next().await {
            for file in request.files {
                let _ = sender.unbounded_send(AppEvent::FileChosen(file));
            }
        }
    });
}

fn forward_system_changes(sender: Sender) {
    let mut changes = system::changes();
    wasdk::spawn(async move {
        while let Some(change) = changes.next().await {
            if let SystemChange::PlatformBrightness(brightness) = change {
                let _ = sender.unbounded_send(AppEvent::Brightness(brightness));
            }
        }
    });
}
