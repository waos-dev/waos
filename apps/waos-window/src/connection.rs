//! The window process's connection to WaOS, on its main thread, once WaOS welcomed it: where the
//! program's windows, its menus, its file dialogs, its notifications and the dialogs of WaOS's
//! prompts send what they report, and where what WaOS sends is carried out, the place of each
//! window's layers in WaOS kept until its window opens.
//!
//! The invariant a later editor will break: nothing waits on WaOS but a window in its resize. A
//! message is sent and the call returns; a key's reply arrives later as `KeyReplied` and resolves
//! the key's future, and a key WaOS never answers was not handled, once the connection ends. A
//! window waits in its resize for the program's picture at its new size (`resize_waits`), which
//! the connection's thread records as WaOS says it is shown, so it never reaches here; and every
//! message sent goes by what the wait hangs on, since an answer to a request the program awaits of
//! the app lets resizes wait again.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use futures::future::Shared;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::views::{KeyEvent, RequestId};
use waos::{KeyHandledFuture, ViewRequest};
use waos_window_messages::os::macos::{SendRight, uptime_now};
use waos_window_messages::{
    FromWindow, HostedLayers, KeyId, PROTOCOL, PermissionRequestId, PromptCall, PromptId, ToWindow,
    ViewCall, Welcome, encode,
};

use crate::program_file_dialogs::AskedFileDialog;
use crate::program_menus::AskedContextMenu;
use crate::program_notifications::AskedNotification;
use crate::resize_waits::ResizeWaits;

/// What a message from WaOS leaves for the window process to do, once the connection carried it
/// out.
pub(crate) enum Carried {
    /// Nothing more.
    Done,
    /// The program asked for a window: the window process makes it.
    WindowAsked(ViewRequest),
    /// The program ended, and the connection with it: the window process quits.
    ProgramEnded,
    /// WaOS asks for a dialog of the prompt `prompt`, `size` logical points large, showing its
    /// context `layers`: the window process shows it in a window of its own.
    PromptAsked {
        prompt: PromptId,
        size: [f64; 2],
        layers: HostedLayers,
    },
    /// WaOS's view of the prompt `prompt` asks its dialog for something.
    PromptCalled { prompt: PromptId, call: PromptCall },
    /// The prompt `prompt` was answered or put away: its dialog goes.
    PromptHidden(PromptId),
    /// The program's menu bar, whole: the window process shows it in place of the one before.
    MenuBarSet(Vec<MenuEntry>),
    /// The program opens a context menu: the window process opens it, and answers once it closes.
    ContextMenuAsked(AskedContextMenu),
    /// The program answered the user's quitting its app: the quits asked hear it.
    QuitAnswered(AppExitResponse),
    /// The program opens a file dialog: the window process opens it, and answers once it closes.
    FileDialogAsked(AskedFileDialog),
    /// The program posts or schedules a notification: the window process posts it, and answers
    /// once the system has it.
    NotificationAsked(AskedNotification),
    /// The program takes its notification of this id away: the window process removes it.
    NotificationRemovalAsked(String),
    /// WaOS asks for macOS's question for the program's notifications: the window process puts
    /// it, and answers with this id once macOS has answered.
    NotificationPermissionAsked(PermissionRequestId),
    /// WaOS asks for macOS's permission for the program's notifications to be read again: the
    /// window process reads it, and tells WaOS.
    NotificationPermissionReadingAsked,
}

pub(crate) struct Connection {
    /// The port of WaOS's side of the connection.
    to_waos: SendRight,
    /// The program's name as its user knows it.
    name: String,
    /// When the program's clock started, on the system's uptime clock.
    clock_start: u64,
    /// Where each window's calls go: the queue of the view shown in it.
    windows: RefCell<HashMap<RequestId, UnboundedSender<ViewCall>>>,
    /// Where WaOS keeps the layers of each window asked for and not yet open.
    layers: RefCell<HashMap<RequestId, HostedLayers>>,
    /// The keys sent whose reply has not come.
    keys: RefCell<HashMap<KeyId, oneshot::Sender<bool>>>,
    next_key: Cell<KeyId>,
    /// Resolves once the connection ended: the program ended, or WaOS is gone.
    ended: Shared<oneshot::Receiver<()>>,
    end: RefCell<Option<oneshot::Sender<()>>>,
    /// What a window's wait in its resize hangs on.
    waits: Arc<ResizeWaits>,
}

impl Connection {
    /// The connection WaOS's `welcome` opened, whose port is `to_waos`; `waits` keeps what a
    /// window's wait in its resize hangs on, as the connection's thread hears it from WaOS.
    pub(crate) fn welcomed(
        welcome: Welcome,
        to_waos: SendRight,
        waits: Arc<ResizeWaits>,
    ) -> Connection {
        let (end, ended) = oneshot::channel();
        Connection {
            to_waos,
            name: welcome.name,
            clock_start: welcome.clock_start,
            windows: RefCell::default(),
            layers: RefCell::default(),
            keys: RefCell::default(),
            next_key: Cell::new(0),
            ended: ended.shared(),
            end: RefCell::new(Some(end)),
            waits,
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// What a window's wait in its resize hangs on.
    pub(crate) fn waits(&self) -> &ResizeWaits {
        &self.waits
    }

    /// Now on the program's clock, in nanoseconds.
    pub(crate) fn now(&self) -> u64 {
        uptime_now().saturating_sub(self.clock_start)
    }

    /// Sends `message` to WaOS. One WaOS can no longer take is dropped: the connection is
    /// ending, and the window process hears so.
    pub(crate) fn send(&self, message: FromWindow) {
        self.send_with_ports(message, Vec::new());
    }

    /// Sends `message` to WaOS with `ports`, which are WaOS's once sent, and let go of when not;
    /// answers whether WaOS took it. An answer to a request the program awaits of the app lets
    /// resizes wait again.
    pub(crate) fn send_with_ports(&self, message: FromWindow, ports: Vec<SendRight>) -> bool {
        self.waits.sent(&message);
        match self.to_waos.send(PROTOCOL, &encode(&message), ports) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, ?message, "WaOS did not take a message");
                false
            }
        }
    }

    /// Sends the key `event` of the view in `window`; resolves with whether the program handled
    /// it once WaOS replies, and as not handled if the connection ends first.
    pub(crate) fn key(&self, window: RequestId, event: KeyEvent) -> KeyHandledFuture {
        let key = self.next_key.get();
        self.next_key.set(key + 1);
        let (reply, handled) = oneshot::channel();
        self.keys.borrow_mut().insert(key, reply);
        self.send(FromWindow::Key { window, key, event });
        Box::pin(handled.map(|handled| handled.unwrap_or(false)))
    }

    /// The program's calls on the view in `window` go to `calls` from now on.
    pub(crate) fn route(&self, window: RequestId, calls: UnboundedSender<ViewCall>) {
        self.windows.borrow_mut().insert(window, calls);
    }

    /// The window `window` is gone: calls on it go nowhere.
    pub(crate) fn unroute(&self, window: RequestId) {
        self.windows.borrow_mut().remove(&window);
    }

    /// Where WaOS keeps the layers of the window `window`, as it asked for the window; none once
    /// taken.
    pub(crate) fn take_layers(&self, window: RequestId) -> Option<HostedLayers> {
        self.layers.borrow_mut().remove(&window)
    }

    /// Carries out what WaOS sent; answers what is left for the window process to do.
    pub(crate) fn received(&self, message: ToWindow) -> Carried {
        match message {
            ToWindow::OpenWindow {
                window,
                config,
                layers,
            } => {
                self.layers.borrow_mut().insert(window, layers);
                return Carried::WindowAsked(ViewRequest { id: window, config });
            }
            ToWindow::View { window, call } => self.call(window, call),
            ToWindow::KeyReplied { key, handled } => self.key_replied(key, handled),
            ToWindow::ProgramEnded => {
                self.end();
                return Carried::ProgramEnded;
            }
            ToWindow::ShowPrompt {
                prompt,
                size,
                layers,
            } => {
                return Carried::PromptAsked {
                    prompt,
                    size,
                    layers,
                };
            }
            ToWindow::Prompt { prompt, call } => return Carried::PromptCalled { prompt, call },
            ToWindow::HidePrompt { prompt } => return Carried::PromptHidden(prompt),
            ToWindow::SetMenuBar(entries) => return Carried::MenuBarSet(entries),
            ToWindow::ShowContextMenu {
                menu,
                window,
                at,
                entries,
            } => {
                return Carried::ContextMenuAsked(AskedContextMenu {
                    menu,
                    window,
                    at,
                    entries,
                });
            }
            ToWindow::QuitAnswered(response) => return Carried::QuitAnswered(response),
            ToWindow::OpenFileDialog {
                dialog,
                parent,
                kind,
            } => {
                return Carried::FileDialogAsked(AskedFileDialog {
                    dialog,
                    parent,
                    kind,
                });
            }
            ToWindow::PostNotification {
                post,
                notification,
                at,
            } => {
                return Carried::NotificationAsked(AskedNotification {
                    post,
                    notification,
                    at,
                });
            }
            ToWindow::RemoveNotification { id } => return Carried::NotificationRemovalAsked(id),
            ToWindow::RequestNotificationPermission { request } => {
                return Carried::NotificationPermissionAsked(request);
            }
            ToWindow::ReadNotificationPermission => {
                return Carried::NotificationPermissionReadingAsked;
            }
            // The connection's thread records it as it arrives, for a window waiting on the main
            // thread, and hands it on to nothing.
            ToWindow::PictureShown { .. } => {}
            ToWindow::Welcome(_) | ToWindow::Refused(_) => {
                tracing::warn!(?message, "WaOS answered a hello twice");
            }
        }
        Carried::Done
    }

    fn call(&self, window: RequestId, call: ViewCall) {
        match self.windows.borrow().get(&window) {
            Some(calls) => {
                let _ = calls.unbounded_send(call);
            }
            None => tracing::debug!(window, ?call, "a call on a window that is gone"),
        }
    }

    fn key_replied(&self, key: KeyId, handled: bool) {
        if let Some(reply) = self.keys.borrow_mut().remove(&key) {
            let _ = reply.send(handled);
        }
    }

    /// The connection ended: every key still waiting was not handled, nothing waits for a
    /// picture, and the windows close.
    pub(crate) fn end(&self) {
        self.keys.borrow_mut().clear();
        self.waits.end();
        if let Some(end) = self.end.borrow_mut().take() {
            let _ = end.send(());
        }
    }

    /// Resolves once the connection ended.
    pub(crate) fn ended(&self) -> impl Future<Output = ()> + 'static {
        self.ended.clone().map(|_| ())
    }
}

#[cfg(test)]
pub(crate) mod tests;
