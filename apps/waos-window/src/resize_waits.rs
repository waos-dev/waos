//! A window's wait in its resize for the program's picture at the new size (decision 55 of
//! `docs/proposals/2026-10-17-the-waos-project.md`): the window sends its view's new size to WaOS
//! and waits there, inside AppKit's step and before macOS shows the new size, with no timeout,
//! until WaOS says the program's picture at that size is shown, so the window's new size and that
//! picture show together, as Flutter's macOS embedder waits in its view's `setFrameSize:`
//! (`ResizeSynchronizer.swift`). Nothing waits before a view's first picture, which WaOS says is
//! shown as it shows it; from then on the window waits in each resize, for a picture WaOS says is
//! shown after it sent the size, as Flutter's waits for a frame committed after the resize began.
//!
//! The wait ends too, the window then showing its new size without the picture at it, once the
//! program ends or WaOS goes, and once the program awaits the app: while WaOS has asked the app on
//! the program's behalf for something the app carries out on its main thread and answers later
//! ([`AwaitedRequest`]), the program may wait for that answer before it draws, so the app cannot
//! wait for its picture. No wait begins while such a request is unanswered, and one that waits ends
//! as such a request arrives. WaOS, which keeps the fence a new size comes with, lets it go by the
//! same rule.
//!
//! The invariant a later editor will break: the wait is woken off the main thread. WaOS's messages
//! arrive on the connection's thread, which records here each picture WaOS says is shown, each
//! request the program awaits, and the connection's end, as they arrive, before it hands anything
//! on to the main thread, which cannot carry out anything while it waits.

use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use waos::bindings::wasdk::platform::views::RequestId;
use waos_window_messages::{
    ContextMenuId, FileDialogId, FromWindow, PermissionRequestId, PostId, PromptId, PromptReport,
    ToWindow,
};

/// A view's size in physical pixels: its width and height.
pub(crate) type PhysicalSize = [u32; 2];

/// What a window's wait in its resize hangs on, shared by the connection's thread, which hears it,
/// and the main thread, which waits and answers.
#[derive(Default)]
pub(crate) struct ResizeWaits {
    waits: Mutex<Waits>,
    /// Told each time what a wait hangs on changes.
    changed: Condvar,
}

#[derive(Default)]
struct Waits {
    /// The views that showed a picture, by their windows, each with the size of the picture WaOS
    /// said it shows since the window last sent a new size; none while it waits for one.
    views: HashMap<RequestId, Option<PhysicalSize>>,
    /// The requests the program awaits the app's answer to, until the app answers.
    requests: HashSet<AwaitedRequest>,
    /// Whether the connection ended: the program ended, or WaOS is gone.
    ended: bool,
}

/// A request WaOS makes of the app on the program's behalf whose answer the program awaits, and
/// which the app carries out on its main thread, by the id its answer names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AwaitedRequest {
    /// A window the program asked for, until it opens or is refused.
    Window(RequestId),
    /// One of WaOS's prompts for the program, whose answer the program awaits, until WaOS hides
    /// its dialog or the dialog closes.
    Prompt(PromptId),
    ContextMenu(ContextMenuId),
    FileDialog(FileDialogId),
    /// A notification's post or schedule, until the system has it.
    Post(PostId),
    /// macOS's question for the program's notifications, until macOS answers.
    PermissionQuestion(PermissionRequestId),
}

/// What a message of WaOS's does to the requests the program awaits the app's answer to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RequestChange {
    /// It asks one.
    Asked(AwaitedRequest),
    /// It settles one, which the program no longer awaits of the app.
    Settled(AwaitedRequest),
    /// Neither.
    Unchanged,
}

/// Why a wait for a picture ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaitEnd {
    /// WaOS said the picture is shown.
    Shown,
    /// The program awaits the app's answer to a request, so it may not draw until the app goes
    /// on.
    Requested,
    /// The connection ended first.
    Ended,
}

impl ResizeWaits {
    /// WaOS says the view in `window` shows the program's picture at `size`: a wait for it ends.
    pub(crate) fn shown(&self, window: RequestId, size: PhysicalSize) {
        self.locked().views.insert(window, Some(size));
        self.changed.notify_all();
    }

    /// What `message`, which WaOS sent, does to the requests the program awaits: a request asked
    /// ends every wait.
    pub(crate) fn heard(&self, message: &ToWindow) {
        match RequestChange::of(message) {
            RequestChange::Asked(request) => self.asked(request),
            RequestChange::Settled(request) => self.settled(request),
            RequestChange::Unchanged => {}
        }
    }

    /// The app sends `message`, which may answer a request the program awaits.
    pub(crate) fn sent(&self, message: &FromWindow) {
        if let Some(request) = AwaitedRequest::answered_by(message) {
            self.settled(request);
        }
    }

    /// The connection ended: every wait ends, and none begins.
    pub(crate) fn end(&self) {
        self.locked().ended = true;
        self.changed.notify_all();
    }

    /// The view in `window` is gone.
    pub(crate) fn forget(&self, window: RequestId) {
        self.locked().views.remove(&window);
    }

    /// Whether a resize of the view in `window` waits for the program's picture at its new size:
    /// once the view showed a picture, while the program awaits no answer of the app and the
    /// connection lasts.
    pub(crate) fn awaits(&self, window: RequestId) -> bool {
        let waits = self.locked();
        !waits.ended && waits.requests.is_empty() && waits.views.contains_key(&window)
    }

    /// The window of the view in `window` is about to send a new size it waits at: only a picture
    /// WaOS says is shown from now on ends the wait for it.
    pub(crate) fn expect(&self, window: RequestId) {
        if let Some(size) = self.locked().views.get_mut(&window) {
            *size = None;
        }
    }

    /// Waits, with no timeout, until WaOS says the view in `window` shows the program's picture at
    /// `size`, since [`expect`](Self::expect); until WaOS asks the app for something the program
    /// awaits; or until the connection ends.
    pub(crate) fn wait(&self, window: RequestId, size: PhysicalSize) -> WaitEnd {
        let started = Instant::now();
        let mut waits = self.locked();
        let end = loop {
            if waits.views.get(&window) == Some(&Some(size)) {
                break WaitEnd::Shown;
            }
            if !waits.requests.is_empty() {
                break WaitEnd::Requested;
            }
            if waits.ended {
                break WaitEnd::Ended;
            }
            waits = self
                .changed
                .wait(waits)
                .unwrap_or_else(PoisonError::into_inner);
        };
        let waited = started.elapsed();
        match end {
            WaitEnd::Shown => {
                tracing::debug!(
                    window,
                    ?size,
                    ?waited,
                    "the resize got the program's picture"
                );
            }
            WaitEnd::Requested => tracing::debug!(
                window,
                ?size,
                ?waited,
                "the resize ended on a request the program awaits"
            ),
            WaitEnd::Ended => tracing::debug!(
                window,
                ?size,
                ?waited,
                "the resize ended with the connection"
            ),
        }
        end
    }

    fn asked(&self, request: AwaitedRequest) {
        self.locked().requests.insert(request);
        self.changed.notify_all();
    }

    fn settled(&self, request: AwaitedRequest) {
        self.locked().requests.remove(&request);
    }

    fn locked(&self) -> MutexGuard<'_, Waits> {
        self.waits.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RequestChange {
    /// What `message` does to the requests the program awaits the app's answer to.
    pub(crate) fn of(message: &ToWindow) -> RequestChange {
        match *message {
            // Each is answered later by the app, from its main thread, and the program's call
            // waits for that answer.
            ToWindow::OpenWindow { window, .. } => {
                RequestChange::Asked(AwaitedRequest::Window(window))
            }
            ToWindow::ShowPrompt { prompt, .. } => {
                RequestChange::Asked(AwaitedRequest::Prompt(prompt))
            }
            ToWindow::ShowContextMenu { menu, .. } => {
                RequestChange::Asked(AwaitedRequest::ContextMenu(menu))
            }
            ToWindow::OpenFileDialog { dialog, .. } => {
                RequestChange::Asked(AwaitedRequest::FileDialog(dialog))
            }
            ToWindow::PostNotification { post, .. } => {
                RequestChange::Asked(AwaitedRequest::Post(post))
            }
            ToWindow::RequestNotificationPermission { request } => {
                RequestChange::Asked(AwaitedRequest::PermissionQuestion(request))
            }
            // A prompt's answer is WaOS's, which takes its dialog away once it has it.
            ToWindow::HidePrompt { prompt } => {
                RequestChange::Settled(AwaitedRequest::Prompt(prompt))
            }
            // A file dialog answered by a setting no longer awaits the app.
            ToWindow::CloseFileDialog { dialog } => {
                RequestChange::Settled(AwaitedRequest::FileDialog(dialog))
            }
            // One-way: the program awaits nothing of the app for them. A call on a view is its
            // cursor, focus, text input or window; a call on a prompt's dialog is its cursor or
            // focus; the menu bar is shown as it is set; a notification is taken away as asked;
            // macOS's permission is told as read, which nothing awaits.
            ToWindow::View { .. }
            | ToWindow::Prompt { .. }
            | ToWindow::SetMenuBar(_)
            | ToWindow::RemoveNotification { .. }
            | ToWindow::ReadNotificationPermission => RequestChange::Unchanged,
            // Answers to the app, which the program awaits nothing of: a key's reply and the
            // program's answer to the user's quitting, which the app awaits of the program.
            ToWindow::KeyReplied { .. } | ToWindow::QuitAnswered(_) => RequestChange::Unchanged,
            // Before the connection, or its end, which ends every wait of its own.
            ToWindow::Welcome(_) | ToWindow::Refused(_) | ToWindow::ProgramEnded => {
                RequestChange::Unchanged
            }
            // What a wait waits for.
            ToWindow::PictureShown { .. } => RequestChange::Unchanged,
        }
    }
}

impl AwaitedRequest {
    /// The request `message`, which the app sends, answers; none for any other message.
    fn answered_by(message: &FromWindow) -> Option<AwaitedRequest> {
        match *message {
            FromWindow::WindowOpened { window, .. } | FromWindow::WindowRefused { window, .. } => {
                Some(AwaitedRequest::Window(window))
            }
            FromWindow::Prompt {
                prompt,
                report: PromptReport::Closed,
            } => Some(AwaitedRequest::Prompt(prompt)),
            FromWindow::ContextMenuClosed { menu, .. } => Some(AwaitedRequest::ContextMenu(menu)),
            FromWindow::FileDialogClosed { dialog, .. } => Some(AwaitedRequest::FileDialog(dialog)),
            FromWindow::NotificationPosted { post, .. } => Some(AwaitedRequest::Post(post)),
            FromWindow::NotificationPermissionAnswered { request, .. } => {
                Some(AwaitedRequest::PermissionQuestion(request))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    use waos::bindings::wasdk::platform::views::Cursor;
    use waos_window_messages::ViewCall;

    use super::*;

    const WINDOW: RequestId = 4;
    const SHOWN: PhysicalSize = [1600, 1200];
    const RESIZED: PhysicalSize = [2000, 1200];

    /// How long a test lets a wait go on before it says the wait has not ended.
    const MEANWHILE: Duration = Duration::from_millis(50);

    /// Waits of which the view in `WINDOW` shows a picture at `SHOWN`, its window about to send a
    /// new size.
    fn resizing() -> Arc<ResizeWaits> {
        let waits = Arc::new(ResizeWaits::default());
        waits.shown(WINDOW, SHOWN);
        waits.expect(WINDOW);
        waits
    }

    /// Waits in `waits` for `RESIZED` on a thread of its own, as the main thread does; answers
    /// whether it ended, and how.
    fn waiting(waits: &Arc<ResizeWaits>) -> (Arc<AtomicBool>, thread::JoinHandle<WaitEnd>) {
        let ended = Arc::new(AtomicBool::new(false));
        let (waits, ending) = (Arc::clone(waits), Arc::clone(&ended));
        let waiter = thread::spawn(move || {
            let end = waits.wait(WINDOW, RESIZED);
            ending.store(true, Ordering::SeqCst);
            end
        });
        (ended, waiter)
    }

    fn context_menu(menu: ContextMenuId) -> ToWindow {
        ToWindow::ShowContextMenu {
            menu,
            window: WINDOW,
            at: (0.0, 0.0),
            entries: Vec::new(),
        }
    }

    #[test]
    fn nothing_waits_before_a_views_first_picture() {
        let waits = ResizeWaits::default();
        assert!(!waits.awaits(WINDOW));
        waits.shown(WINDOW, SHOWN);
        assert!(waits.awaits(WINDOW), "once it showed one");
        assert!(!waits.awaits(WINDOW + 1), "another view has shown none");
        waits.forget(WINDOW);
        assert!(!waits.awaits(WINDOW), "a view gone shows none");
    }

    #[test]
    fn a_wait_ends_on_the_picture_of_its_size_alone() {
        let waits = resizing();
        let (ended, waiter) = waiting(&waits);
        waits.shown(WINDOW + 1, RESIZED);
        waits.shown(WINDOW, [1800, 1200]);
        thread::sleep(MEANWHILE);
        assert!(
            !ended.load(Ordering::SeqCst),
            "another view's picture, and one of another size, end nothing"
        );
        waits.shown(WINDOW, RESIZED);
        assert_eq!(waiter.join().unwrap(), WaitEnd::Shown);
    }

    #[test]
    fn a_wait_ends_only_on_a_picture_said_shown_after_the_new_size() {
        let waits = Arc::new(ResizeWaits::default());
        waits.shown(WINDOW, RESIZED);
        waits.expect(WINDOW);
        let (ended, waiter) = waiting(&waits);
        thread::sleep(MEANWHILE);
        assert!(
            !ended.load(Ordering::SeqCst),
            "what WaOS said before is no answer"
        );
        waits.shown(WINDOW, RESIZED);
        assert_eq!(waiter.join().unwrap(), WaitEnd::Shown);
    }

    #[test]
    fn a_wait_ends_with_the_connection() {
        let waits = resizing();
        let (ended, waiter) = waiting(&waits);
        thread::sleep(MEANWHILE);
        assert!(!ended.load(Ordering::SeqCst), "it has no timeout");
        waits.end();
        assert_eq!(waiter.join().unwrap(), WaitEnd::Ended);
        assert!(!waits.awaits(WINDOW), "and none begins once it ended");
    }

    #[test]
    fn a_wait_ends_on_a_request_the_program_awaits_and_none_begins_until_it_is_answered() {
        let waits = resizing();
        let (ended, waiter) = waiting(&waits);
        waits.heard(&ToWindow::View {
            window: WINDOW,
            call: ViewCall::SetCursor(Cursor::Text),
        });
        thread::sleep(MEANWHILE);
        assert!(!ended.load(Ordering::SeqCst), "a one-way call ends nothing");
        waits.heard(&context_menu(7));
        assert_eq!(waiter.join().unwrap(), WaitEnd::Requested);
        assert!(
            !waits.awaits(WINDOW),
            "no resize waits while the program awaits the menu"
        );
        waits.sent(&FromWindow::ContextMenuClosed {
            menu: 7,
            chosen: None,
        });
        assert!(
            waits.awaits(WINDOW),
            "once the app answered, resizes wait again"
        );
    }

    #[test]
    fn a_prompt_is_awaited_until_its_dialog_goes() {
        let waits = resizing();
        let shown = ToWindow::ShowPrompt {
            prompt: 2,
            size: [440.0, 320.0],
            layers: waos_window_messages::HostedLayers { context_id: 1 },
        };
        waits.heard(&shown);
        assert!(!waits.awaits(WINDOW));
        waits.heard(&ToWindow::HidePrompt { prompt: 2 });
        assert!(waits.awaits(WINDOW), "WaOS took its dialog away");
        waits.heard(&shown);
        waits.sent(&FromWindow::Prompt {
            prompt: 2,
            report: PromptReport::Closed,
        });
        assert!(waits.awaits(WINDOW), "the dialog closed otherwise");
    }

    #[test]
    fn only_what_the_program_awaits_of_the_app_is_a_request() {
        assert_eq!(
            RequestChange::of(&ToWindow::RequestNotificationPermission { request: 3 }),
            RequestChange::Asked(AwaitedRequest::PermissionQuestion(3))
        );
        for one_way in [
            ToWindow::SetMenuBar(Vec::new()),
            ToWindow::RemoveNotification { id: "a".to_owned() },
            ToWindow::ReadNotificationPermission,
            ToWindow::KeyReplied {
                key: 1,
                handled: true,
            },
        ] {
            assert_eq!(
                RequestChange::of(&one_way),
                RequestChange::Unchanged,
                "{one_way:?}"
            );
        }
    }
}
