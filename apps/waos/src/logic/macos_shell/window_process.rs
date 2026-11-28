//! One app's window process, as WaOS serves it once it said hello, and the app's instance attached
//! to it once that first reaches it, never another after: each window the instance asks for is
//! opened there, once WaOS made the canvas its view shows (`canvases`), with where that
//! canvas's layers are; each view shown there is added to the instance, its hardware buffers
//! presented to the canvas, in WaOS, and its calls handed back as `ViewCall`s; what the window
//! reports of a view goes to the view, a new size placing the canvas at it, at once, or, where the
//! window waits in the resize, with the instance's picture at that size under the window's fence,
//! the window then hearing the picture is shown (`canvas_pictures`); a key's reply goes back once
//! the instance gives it; and when a window goes, its view and its canvas go with it, as every one
//! does when the window process goes. The instance's menu bar is shown there, the app's while it is
//! in front, sent whole each time the instance sets it and as the window process connects, and a
//! choice in it goes back to the instance by its id; its context menus open there at a point of a
//! view shown there, each answering the instance's call once it closes, or with none once the window
//! process goes (`awaited_answers`). WaOS's prompts for the app are shown in dialogs there, and
//! what a dialog reports goes on to the main thread, where WaOS's view of the prompt is
//! ([`DialogReport`]); when the window process goes, every dialog it showed is reported closed. The
//! user's quitting the app asks the instance, as WaOS's own Quit does, and the window process
//! hears its answer; an instance that agrees is stopped, and its window process ends with it. Its file dialogs open
//! there, a sheet on the window showing the view the instance names, or on their own where it names
//! none or no window there shows it, each answering the instance's call with the paths the user
//! picked once it closes, which the engine mounts, or with why none were once the window process
//! goes. Its notifications are posted, scheduled and removed there, under the app's bundle, each
//! post answering the instance's call once the system has it, or with why not once the window
//! process goes, and what the user does to one there goes to the instance. macOS's own question for
//! them is put there too, answering what macOS says of the app's notifications then, or nothing
//! once the window process goes; macOS's permission for them is read there again when WaOS asks;
//! and whatever the app says macOS said of them, as WaOS welcomed it, as the question is answered,
//! as it comes to the front, as WaOS asks and as a post is answered, is kept for the engine, which
//! reads it with the instance's permission, and goes on to the main thread, where WaOS's settings
//! page for the app says so ([`NotificationPermissions`]).
//!
//! Everything WaOS keeps of the window process while it is there, its windows, its prompts'
//! dialogs and what it was asked and owes an answer to, is one [`Connected`], taken whole as it
//! goes. A window process no instance attached to shows WaOS's prompts alone: what the window
//! process says of an instance (a choice in the menu bar, files opened with the app, a click on a
//! notification) goes nowhere then, and the user quitting the app is answered at once.
//!
//! The invariants a later editor will break: once the window process is gone nothing new is asked
//! of it, and everything it owed is answered exactly once, a window the instance asked for among
//! it, since every ask is checked and kept under the one lock that taking `Connected` holds; and
//! nothing here waits on the window process or on the main thread. A message is sent and the call
//! returns, a canvas is made, placed and let go by a call to the main thread, a dialog's report and what macOS said are sent on to it, a key's reply
//! is sent from the runtime the window process is served on, once the instance answers, as the
//! answer to a quit is, and the answer to a context menu, a file dialog, a notification's post or
//! macOS's question resolves the instance's call when it arrives.

use std::collections::{HashMap, HashSet};
use std::future::ready;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::task::{Context, Poll};
use std::time::SystemTime;

use futures::FutureExt;
use futures::channel::mpsc::UnboundedSender;

use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::notifications::NotificationError;
use waos::bindings::wasdk::platform::views::{KeyEvent, Metrics, RequestId, ViewError, ViewId};
use waos::{
    FileDialogFuture, HeldModifiers, MenuChosenFuture, Notification, NotificationFuture,
    PlacedView, PlacedWindow, ProgramView, RunningProgram, SystemPermissionState, ViewRequest,
};
use waos_package::ProgramId;
use waos_program_window::{CanvasLayer, CanvasView, ViewCalls, WindowCalls};
use waos_window_messages::os::macos::SendRight;
use waos_window_messages::{
    FileDialog, FileDialogId, FromWindow, HostedLayers, KeyId, OpenedWindow, PromptCall, PromptId,
    PromptReport, ToWindow, ViewReport,
};

use super::awaited_answers::{APP_GONE, AppGone, AwaitedAnswer, AwaitedAnswers};
use super::canvas_pictures::CanvasPictures;
use super::canvases::{CanvasKey, CanvasesOnRequest};
use super::file_dialogs::failed;
use crate::logic::notification_permissions::NotificationPermissions;

/// What sends messages to the window process, until WaOS closes it: nothing is sent after, and the
/// window process, left with none of WaOS's rights to its port, hears its connection end.
#[derive(Clone)]
pub(crate) struct SendToWindow(Arc<Mutex<Option<SendMessage>>>);

/// Sends one message to the window process: over its port, or a test's stand-in.
type SendMessage = Box<dyn Fn(ToWindow) + Send + Sync>;

impl SendToWindow {
    /// Sends each message with `send`, until closed.
    pub(crate) fn new(send: impl Fn(ToWindow) + Send + Sync + 'static) -> SendToWindow {
        SendToWindow(Arc::new(Mutex::new(Some(Box::new(send)))))
    }

    /// Sends `message`; nothing once closed.
    pub(crate) fn send(&self, message: ToWindow) {
        if let Some(send) = self.sending().as_ref() {
            send(message);
        }
    }

    /// Sends nothing more, letting go of what sends, the window process's port among it.
    pub(crate) fn close(&self) {
        self.sending().take();
    }

    fn sending(&self) -> MutexGuard<'_, Option<SendMessage>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the dialog of one of WaOS's prompts reported, on its way to the main thread.
pub(crate) struct DialogReport {
    pub(crate) prompt: PromptId,
    pub(crate) report: PromptReport,
}

/// Where the dialogs' reports go, on their way to the main thread.
pub(crate) type DialogReports = UnboundedSender<DialogReport>;

/// Whether one of a program's windows is in sight, as its window process says, on its way to the
/// main thread, where the activity that keeps macOS from throttling WaOS follows it.
#[derive(Debug)]
pub(crate) struct WindowPresence {
    pub(crate) program: ProgramId,
    pub(crate) window: RequestId,
    pub(crate) in_sight: bool,
}

/// Where whether each of a program's windows is in sight goes, on its way to the main thread.
pub(crate) type WindowPresences = UnboundedSender<WindowPresence>;

pub(crate) struct WindowProcess {
    app: ProgramId,
    /// The app's instance it serves, once that reached it; never another after.
    instance: OnceLock<RunningProgram>,
    send: SendToWindow,
    canvases: Arc<CanvasesOnRequest>,
    /// Where a key's reply is awaited.
    runtime: tokio::runtime::Handle,
    /// What WaOS keeps of the window process while it is there; none once it is gone, after which
    /// a canvas made is let go at once, its request failed, a prompt shown is closed at once, and
    /// whatever else is asked answers at once as an app that is gone.
    connected: Mutex<Option<Connected>>,
    dialog_reports: DialogReports,
    /// Where what the app says macOS said of the program's notifications is kept.
    notification_permissions: NotificationPermissions,
    /// Whether each of the program's windows is in sight, on its way to the main thread.
    window_presences: WindowPresences,
    /// Held while the program's menu bar is read and sent, so the bar sent last is the one it set
    /// last.
    menu_bar_sending: Mutex<()>,
    /// Held while WaOS asks the app for something the program awaits, letting every resize waiting
    /// go first, and while it takes a new size a window waits at, so no resize is kept waiting
    /// behind such a request (`canvas_pictures`).
    asking: Mutex<()>,
}

/// What WaOS keeps of a window process while it is there: each window asked of it, each prompt
/// shown in it, and each answer awaited from it.
#[derive(Default)]
struct Connected {
    /// Each window asked of the window process, by the request it was made for.
    windows: HashMap<RequestId, AppWindow>,
    /// The prompts shown in dialogs there, until hidden or closed.
    prompts: HashSet<PromptId>,
    /// The context menus open there, each answering the item chosen in it.
    context_menus: AwaitedAnswers<Option<u32>>,
    /// The file dialogs open there, each answering the paths picked in it, or why none were.
    file_dialogs: AwaitedAnswers<Result<Vec<PathBuf>, DialogError>>,
    /// The notifications being posted or scheduled there, each answering whether the system has
    /// it.
    notifications: AwaitedAnswers<Result<(), NotificationError>>,
    /// macOS's questions for the program's notifications being put there, each answering whether
    /// macOS refuses them.
    notification_permissions: AwaitedAnswers<SystemPermissionState>,
}

impl Connected {
    /// Whether the program awaits the app's answer to something WaOS asked of it, which the app
    /// carries out on its main thread: a window asked for and not yet open, a prompt's dialog
    /// shown, a context menu, a file dialog, a notification's post or macOS's question for the
    /// program's notifications. The window process keeps the same list (its `resize_waits`).
    fn owes_answers(&self) -> bool {
        self.windows.values().any(|window| window.view.is_none())
            || !self.prompts.is_empty()
            || self.context_menus.any()
            || self.file_dialogs.any()
            || self.notifications.any()
            || self.notification_permissions.any()
    }
}

/// A window the program asked for in its app, as WaOS keeps it.
struct AppWindow {
    /// The canvas WaOS keeps for it, as the program's pictures reach it, and the key its calls
    /// name it by.
    pictures: Arc<CanvasPictures>,
    key: CanvasKey,
    /// The view shown in it, once it opened.
    view: Option<ProgramView>,
    /// Whether its app says the window is in sight: on screen and not occluded.
    in_sight: bool,
}

impl WindowProcess {
    /// `app`'s window process, which `send` reaches, serving no instance yet, its views' canvases
    /// kept by `canvases`, what its prompts' dialogs report sent to `dialog_reports`, what it says
    /// macOS said of the app's notifications kept in `notification_permissions`, and whether each
    /// of its windows is in sight to `window_presences`; key replies are awaited on `runtime`.
    pub(crate) fn new(
        app: ProgramId,
        send: SendToWindow,
        canvases: Arc<CanvasesOnRequest>,
        dialog_reports: DialogReports,
        notification_permissions: NotificationPermissions,
        window_presences: WindowPresences,
        runtime: tokio::runtime::Handle,
    ) -> Arc<WindowProcess> {
        Arc::new(WindowProcess {
            app,
            instance: OnceLock::new(),
            send,
            canvases,
            runtime,
            connected: Mutex::new(Some(Connected::default())),
            dialog_reports,
            notification_permissions,
            window_presences,
            menu_bar_sending: Mutex::default(),
            asking: Mutex::default(),
        })
    }

    /// Shows WaOS's prompt `prompt` in a dialog of the program's app, `size` logical points large,
    /// showing the layer context `layers`; closed at once once the window process is gone.
    pub(crate) fn show_prompt(&self, prompt: PromptId, size: [f64; 2], layers: HostedLayers) {
        let _asking = self.asking_the_app();
        let mut kept = self.connected();
        let Some(connected) = kept.as_mut() else {
            drop(kept);
            self.dialog_reported(prompt, PromptReport::Closed);
            return;
        };
        connected.prompts.insert(prompt);
        self.send.send(ToWindow::ShowPrompt {
            prompt,
            size,
            layers,
        });
    }

    /// WaOS's view of `prompt` asks its dialog for something.
    pub(crate) fn call_prompt(&self, prompt: PromptId, call: PromptCall) {
        self.send.send(ToWindow::Prompt { prompt, call });
    }

    /// Takes the dialog of `prompt` away, if it is still shown.
    pub(crate) fn hide_prompt(&self, prompt: PromptId) {
        if self.with_connected(|connected| connected.prompts.remove(&prompt)) == Some(true) {
            self.send.send(ToWindow::HidePrompt { prompt });
        }
    }

    /// Whether a prompt is shown in a dialog there.
    pub(crate) fn shows_prompts(&self) -> bool {
        self.with_connected(|connected| !connected.prompts.is_empty()) == Some(true)
    }

    /// The app it is the window process of.
    pub(crate) fn app(&self) -> &ProgramId {
        &self.app
    }

    /// The app's instance it serves, once that reached it.
    pub(crate) fn instance(&self) -> Option<&RunningProgram> {
        self.instance.get()
    }

    /// Whether it serves `program`.
    pub(crate) fn serves(&self, program: &RunningProgram) -> bool {
        self.instance() == Some(program)
    }

    /// `program`, the app's instance, reached it: it serves `program` from now on, unless it
    /// serves another already.
    pub(crate) fn attach(&self, program: RunningProgram) {
        if let Err(program) = self.instance.set(program)
            && !self.serves(&program)
        {
            tracing::warn!(app = %self.app, "a second instance reached the app's window process");
        }
    }

    /// Hands `hear` the instance it serves; nothing where none reached it, which then has nothing
    /// to hear.
    fn to_instance(&self, hear: impl FnOnce(&RunningProgram)) {
        match self.instance() {
            Some(instance) => hear(instance),
            None => tracing::info!(app = %self.app, "no instance of the app runs to hear it"),
        }
    }

    /// Shows the instance's menu bar in its app as the engine keeps it, the one the instance set
    /// last; nothing while it has set none, or none is attached, and the app shows the standard
    /// one.
    pub(crate) fn show_menu_bar(&self) {
        let _sending = self
            .menu_bar_sending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(bar) = self.instance().and_then(RunningProgram::menu_bar) {
            self.send.send(ToWindow::SetMenuBar(bar.to_vec()));
        }
    }

    /// Opens `entries` in the app at `at`, in the physical pixels of the program's view `view`,
    /// and answers the item chosen once the menu closes; none at once where no window there shows
    /// the view, or once the window process is gone.
    pub(crate) fn show_context_menu(
        &self,
        view: ViewId,
        at: (f64, f64),
        entries: &[MenuEntry],
    ) -> MenuChosenFuture {
        let Some(window) = self.window_showing(view) else {
            return Box::pin(ready(None));
        };
        let _asking = self.asking_the_app();
        let Some((menu, answered)) = self.with_connected(|connected| connected.context_menus.ask())
        else {
            return Box::pin(ready(None));
        };
        self.send.send(ToWindow::ShowContextMenu {
            menu,
            window,
            at,
            entries: entries.to_vec(),
        });
        Box::pin(answered.map(|chosen| chosen.unwrap_or(None)))
    }

    /// Opens the file dialog `kind` in the app, a sheet on the window showing the program's view
    /// `parent`, or on its own where it names none or no window there shows it, and answers the
    /// paths the user picked once it closes; with why none were once the window process is gone.
    /// Dropping the returned future before it closes tells the window process to close the dialog.
    pub(crate) fn open_file_dialog(
        &self,
        parent: Option<ViewId>,
        kind: FileDialog,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        let _asking = self.asking_the_app();
        let Some((dialog, picked)) = self.with_connected(|connected| connected.file_dialogs.ask())
        else {
            return Box::pin(ready(Err(failed(APP_GONE))));
        };
        self.send.send(ToWindow::OpenFileDialog {
            dialog,
            parent: parent.and_then(|view| self.window_showing(view)),
            kind,
        });
        Box::pin(WaitFileDialog {
            dialog,
            send: self.send.clone(),
            picked,
        })
    }

    /// Posts the program's `notification` in the app, under its bundle, now or at `at`, and
    /// answers once the system has it, or why not; with why once the window process is gone.
    pub(crate) fn post_notification(
        &self,
        notification: Notification,
        at: Option<SystemTime>,
    ) -> NotificationFuture {
        let app_gone = || NotificationError::Failed(APP_GONE.to_owned());
        let _asking = self.asking_the_app();
        let Some((post, posted)) = self.with_connected(|connected| connected.notifications.ask())
        else {
            return Box::pin(ready(Err(app_gone())));
        };
        self.send.send(ToWindow::PostNotification {
            post,
            notification,
            at,
        });
        Box::pin(posted.map(move |posted| posted.unwrap_or_else(|AppGone| Err(app_gone()))))
    }

    /// Takes the program's notification `id` away in the app, shown or scheduled.
    pub(crate) fn remove_notification(&self, id: String) {
        self.send.send(ToWindow::RemoveNotification { id });
    }

    /// Puts macOS's own question for the program's notifications in the app, which asks the user
    /// only if they were never asked, and answers what macOS says of them once it has answered;
    /// nothing once the window process is gone.
    pub(crate) fn request_notification_permission(
        &self,
    ) -> impl Future<Output = Option<SystemPermissionState>> + Send + 'static + use<> {
        let _asking = self.asking_the_app();
        let asked = self.with_connected(|connected| connected.notification_permissions.ask());
        let asked = asked.map(|(request, answered)| {
            self.send
                .send(ToWindow::RequestNotificationPermission { request });
            answered
        });
        async move { asked?.await.ok() }
    }

    /// Has the app read macOS's permission for the program's notifications again, without asking
    /// the user; it tells what it read as it tells every reading.
    pub(crate) fn read_notification_permission(&self) {
        self.send.send(ToWindow::ReadNotificationPermission);
    }

    /// The program asked for a window as `request` says: WaOS makes its canvas, then asks the
    /// window process for the window ([`canvas_made`](Self::canvas_made)).
    pub(crate) fn open_window(self: &Arc<Self>, request: ViewRequest) {
        self.canvases.make(Arc::clone(self), request);
    }

    /// The canvas of the window `request` asks for is made, under `key`, its layers where `layers`
    /// says: the window process makes the window; once it is gone, the canvas is let go and the
    /// request fails.
    pub(crate) fn canvas_made(
        &self,
        key: CanvasKey,
        request: ViewRequest,
        canvas: Arc<CanvasLayer>,
        layers: HostedLayers,
    ) {
        let _asking = self.asking_the_app();
        let mut kept = self.connected();
        let Some(connected) = kept.as_mut() else {
            drop(kept);
            self.canvases.remove(key);
            self.to_instance(|instance| {
                instance.view_request_failed(request.id, ViewError::Failed(APP_GONE.to_owned()));
            });
            return;
        };
        let pictures = CanvasPictures::new(
            request.id,
            canvas,
            Arc::clone(&self.canvases),
            key,
            self.send.clone(),
        );
        let window = AppWindow {
            pictures,
            key,
            view: None,
            in_sight: false,
        };
        connected.windows.insert(request.id, window);
        drop(kept);
        self.send.send(ToWindow::OpenWindow {
            window: request.id,
            config: request.config,
            layers,
        });
    }

    /// No canvas could be made for the window `request` asks for, for `reason`: it fails.
    pub(crate) fn canvas_refused(&self, request: ViewRequest, reason: &str) {
        let error = ViewError::Failed(format!("WaOS could not keep the window's canvas: {reason}"));
        self.to_instance(|instance| instance.view_request_failed(request.id, error));
    }

    /// WaOS ends the window process, its instance having ended or nothing being shown there: it
    /// closes its windows and ends. Nothing is sent to one already gone, as when the instance
    /// ended because its window process did.
    pub(crate) fn end(&self) {
        if self.connected().is_some() {
            self.send.send(ToWindow::ProgramEnded);
        }
    }

    /// WaOS no longer serves the window process: nothing more is sent to it, and, its port let
    /// go, it hears its connection end and quits.
    pub(crate) fn close(&self) {
        self.send.close();
    }

    /// Carries out what the window process sent, with the ports that came with it.
    pub(crate) fn received(&self, message: FromWindow, ports: Vec<SendRight>) {
        tracing::trace!(app = %self.app, ?message);
        match message {
            FromWindow::Hello(_) => {
                tracing::warn!(app = %self.app, "a second hello");
            }
            FromWindow::WindowOpened { window, opened } => self.add_view(window, opened),
            FromWindow::WindowRefused { window, error } => {
                self.forget_window(window);
                self.to_instance(|instance| instance.view_request_failed(window, error));
            }
            FromWindow::View {
                window,
                report: ViewReport::MetricsChanged(metrics),
            } => self.resized(window, metrics, ports.into_iter().next()),
            FromWindow::View { window, report } => {
                if let ViewReport::PresenceChanged(presence) = &report {
                    self.window_in_sight_changed(window, presence.in_sight());
                }
                if let Some(view) = self.view(window) {
                    report_to(&view, report);
                }
            }
            FromWindow::Key { window, key, event } => self.key(window, key, event),
            FromWindow::WindowClosed { window } => self.forget_window(window),
            FromWindow::Opened(request) => {
                self.to_instance(|instance| instance.open_requested(request));
            }
            FromWindow::Prompt { prompt, report } => self.dialog_reported(prompt, report),
            FromWindow::MenuBarChosen(id) => self.to_instance(|instance| instance.menu_chosen(id)),
            FromWindow::ContextMenuClosed { menu, chosen } => {
                self.with_connected(|connected| connected.context_menus.answered(menu, chosen));
            }
            FromWindow::QuitRequested => self.quit_requested(),
            FromWindow::FileDialogClosed { dialog, picked } => {
                self.with_connected(|connected| connected.file_dialogs.answered(dialog, picked));
            }
            FromWindow::NotificationPosted { post, posted } => {
                if let Some(state) = said_by_post(&posted) {
                    self.macos_said(state);
                }
                self.with_connected(|connected| connected.notifications.answered(post, posted));
            }
            FromWindow::NotificationActivated(activation) => {
                self.to_instance(|instance| instance.notification_activated(activation));
            }
            FromWindow::NotificationPermission { state } => self.macos_said(state),
            FromWindow::NotificationPermissionAnswered { request, state } => {
                self.macos_said(state);
                self.with_connected(|connected| {
                    connected.notification_permissions.answered(request, state);
                });
            }
        }
    }

    /// The app said what macOS says of its notifications: kept, for its instances' permission to
    /// read, and for WaOS's settings page for the app to hear.
    fn macos_said(&self, state: SystemPermissionState) {
        self.notification_permissions.said(self.app.clone(), state);
    }

    /// The user quit the app: its instance is asked, as WaOS's own Quit asks it, and the window
    /// process hears its answer; an instance that agrees is stopped, and its window process ends
    /// with it. One with no instance, showing WaOS's prompts alone, is answered at once that it may
    /// quit.
    fn quit_requested(&self) {
        let Some(program) = self.instance().cloned() else {
            self.send
                .send(ToWindow::QuitAnswered(AppExitResponse::Exit));
            return;
        };
        let asked = program.request_exit();
        let send = self.send.clone();
        self.runtime.spawn(async move {
            let response = asked.await;
            send.send(ToWindow::QuitAnswered(response));
            if response == AppExitResponse::Exit {
                program.stop();
            }
        });
    }

    /// The window process is gone: what WaOS kept of it is taken, so nothing is asked of it after,
    /// and everything it owed is answered: every view shown there is taken away, every canvas let
    /// go, every window asked for and not yet open failed, every context menu answered none, every
    /// file dialog answered with why nothing was picked, every notification being posted answered
    /// with why it was not, every question of macOS's answered nothing, and every prompt's dialog
    /// closed.
    pub(crate) fn gone(&self) {
        let Some(connected) = self.connected().take() else {
            return;
        };
        let Connected {
            windows,
            prompts,
            context_menus,
            file_dialogs,
            notifications,
            notification_permissions,
        } = connected;
        for (request, window) in windows {
            let opened = window.view.is_some();
            self.let_go(request, window);
            if !opened {
                self.to_instance(|instance| {
                    instance.view_request_failed(request, ViewError::Failed(APP_GONE.to_owned()));
                });
            }
        }
        context_menus.app_gone();
        file_dialogs.app_gone();
        notifications.app_gone();
        notification_permissions.app_gone();
        for prompt in prompts {
            self.dialog_reported(prompt, PromptReport::Closed);
        }
    }

    /// The dialog of `prompt` reported `report`, which goes on to the main thread; a dialog closed
    /// is no longer shown.
    fn dialog_reported(&self, prompt: PromptId, report: PromptReport) {
        if matches!(report, PromptReport::Closed) {
            self.with_connected(|connected| connected.prompts.remove(&prompt));
        }
        let _ = self
            .dialog_reports
            .unbounded_send(DialogReport { prompt, report });
    }

    /// Adds the view shown in the window made for `window`, its hardware buffers presented to the
    /// window's canvas, placed at its size, and its calls going back as `ViewCall`s on that window,
    /// its size limits among them where the window said it has them; the window starts maximized
    /// or not, and with the system's setting for a double-click on a title bar, as it said.
    fn add_view(&self, window: RequestId, opened: OpenedWindow) {
        let (Some(pictures), Some(instance)) = (self.pictures_of(window), self.instance()) else {
            tracing::warn!(window, "a window opened that WaOS did not ask for");
            return;
        };
        pictures.resize_at_once(opened.metrics, None);
        let send = self.send.clone();
        let calls = ViewCalls::new(move |call| send.send(ToWindow::View { window, call }));
        let view = instance.add_view(
            window,
            PlacedView {
                view: Arc::new(CanvasView::new(pictures, calls.clone())),
                text_input: Some(Arc::new(calls.clone())),
                // A Mac's window has no system bars over it.
                system_bars: None,
                window: Some(PlacedWindow {
                    window: Arc::new(WindowCalls::new(calls, opened.size_limits)),
                    frame: opened.frame,
                    maximized: opened.maximized,
                    fullscreen: opened.fullscreen,
                    title_bar_double_click_action: opened.title_bar_double_click_action,
                }),
                metrics: opened.metrics,
                screen: opened.screen,
                presence: opened.presence,
            },
        );
        tracing::info!(
            app = %self.app,
            window,
            view = view.id(),
            "a window was opened in the app's window process"
        );
        self.with_connected(|connected| {
            if let Some(shown) = connected.windows.get_mut(&window) {
                shown.view = Some(view);
            }
        });
        self.window_in_sight_changed(window, opened.presence.in_sight());
    }

    /// The view in `window` takes a new size: its canvas is resized to it, with the program's
    /// picture at it where the window sent `fence` and waits for that picture, and then the
    /// program hears of it. While the program awaits the app's answer to a request, the window
    /// waits for nothing, its wait having ended as the request arrived, and the canvas is laid out
    /// at once. A window that waits for a view WaOS keeps no canvas for is answered at once, so it
    /// never waits for what nothing will show; none does once it heard its view shows a picture.
    fn resized(&self, window: RequestId, metrics: Metrics, fence: Option<SendRight>) {
        {
            let _asking = self.asking();
            let waits = fence.is_some() && !self.owes_answers();
            match (self.pictures_of(window), fence) {
                (Some(pictures), Some(fence)) if waits => pictures.resize(metrics, fence),
                (Some(pictures), fence) => pictures.resize_at_once(metrics, fence),
                (None, _) if waits => self.send.send(ToWindow::PictureShown {
                    window,
                    size: [metrics.width, metrics.height],
                }),
                (None, _) => {}
            }
        }
        if let Some(view) = self.view(window) {
            view.metrics_changed(metrics);
        }
    }

    /// Whether the program awaits the app's answer to something WaOS asked of it
    /// ([`Connected::owes_answers`]); nothing once the app is gone.
    fn owes_answers(&self) -> bool {
        self.with_connected(|connected| connected.owes_answers()) == Some(true)
    }

    /// WaOS is about to ask the app for something the program awaits: every resize a window waits
    /// in lets its fence go first, its canvas laid out at once, since the window's wait ends as
    /// the request arrives; held until it is asked.
    fn asking_the_app(&self) -> MutexGuard<'_, ()> {
        let asking = self.asking();
        let waiting: Vec<Arc<CanvasPictures>> = self
            .with_connected(|connected| {
                (connected.windows.values())
                    .map(|window| Arc::clone(&window.pictures))
                    .collect()
            })
            .unwrap_or_default();
        for pictures in waiting {
            pictures.let_resize_go();
        }
        asking
    }

    fn asking(&self) -> MutexGuard<'_, ()> {
        self.asking.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The window `window` is gone, or never came: it is let go ([`let_go`](Self::let_go)).
    fn forget_window(&self, window: RequestId) {
        let gone = self.with_connected(|connected| connected.windows.remove(&window));
        if let Some(gone) = gone.flatten() {
            self.let_go(window, gone);
        }
    }

    /// `gone`, the window made for `window`, is no longer kept: its view is taken away, and its
    /// canvas, which nothing will show again, let go.
    fn let_go(&self, window: RequestId, gone: AppWindow) {
        if gone.in_sight {
            self.tell_in_sight(window, false);
        }
        gone.pictures.window_gone();
        self.canvases.remove(gone.key);
        if let Some(view) = gone.view {
            self.to_instance(|instance| instance.remove_view(view.id()));
        }
    }

    /// Keeps whether the window `window` is in sight, telling the main thread when it changes.
    fn window_in_sight_changed(&self, window: RequestId, in_sight: bool) {
        let changed = self.with_connected(|connected| {
            (connected.windows.get_mut(&window))
                .is_some_and(|shown| std::mem::replace(&mut shown.in_sight, in_sight) != in_sight)
        });
        if changed == Some(true) {
            self.tell_in_sight(window, in_sight);
        }
    }

    /// Tells the main thread whether the window `window` is in sight now, for the activity that
    /// keeps macOS from throttling WaOS.
    fn tell_in_sight(&self, window: RequestId, in_sight: bool) {
        let _ = self.window_presences.unbounded_send(WindowPresence {
            program: self.app.clone(),
            window,
            in_sight,
        });
    }

    /// Hands the program the key; its reply goes back once it gives one.
    fn key(&self, window: RequestId, key: KeyId, event: KeyEvent) {
        let Some(view) = self.view(window) else {
            self.send.send(ToWindow::KeyReplied {
                key,
                handled: false,
            });
            return;
        };
        let handled = view.key(event);
        let send = self.send.clone();
        self.runtime.spawn(async move {
            let handled = handled.await;
            send.send(ToWindow::KeyReplied { key, handled });
        });
    }

    fn view(&self, window: RequestId) -> Option<ProgramView> {
        self.with_connected(|connected| connected.windows.get(&window)?.view.clone())?
    }

    /// The window that shows the program's view `view`, by the request it was made for.
    fn window_showing(&self, view: ViewId) -> Option<RequestId> {
        self.with_connected(|connected| {
            connected.windows.iter().find_map(|(window, shown)| {
                let shows = shown.view.as_ref()?.id() == view;
                shows.then_some(*window)
            })
        })?
    }

    fn pictures_of(&self, window: RequestId) -> Option<Arc<CanvasPictures>> {
        self.with_connected(|connected| {
            Some(Arc::clone(&connected.windows.get(&window)?.pictures))
        })?
    }

    /// What `use_connected` answers of what WaOS keeps of the window process; none once it is
    /// gone.
    fn with_connected<R>(&self, use_connected: impl FnOnce(&mut Connected) -> R) -> Option<R> {
        self.connected().as_mut().map(use_connected)
    }

    fn connected(&self) -> MutexGuard<'_, Option<Connected>> {
        self.connected
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

struct WaitFileDialog {
    dialog: FileDialogId,
    send: SendToWindow,
    picked: AwaitedAnswer<Result<Vec<PathBuf>, DialogError>>,
}

impl Future for WaitFileDialog {
    type Output = Result<Vec<PathBuf>, DialogError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.picked
            .poll_unpin(cx)
            .map(|picked| picked.unwrap_or_else(|AppGone| Err(failed(APP_GONE))))
    }
}

impl Drop for WaitFileDialog {
    fn drop(&mut self) {
        if self.picked.cancel() {
            self.send.send(ToWindow::CloseFileDialog {
                dialog: self.dialog,
            });
        }
    }
}

/// What the app's answer `posted` says of macOS: `denied`, that it denies the program its
/// notifications; taken, that it allows them; a failure, nothing.
fn said_by_post(posted: &Result<(), NotificationError>) -> Option<SystemPermissionState> {
    match posted {
        Ok(()) => Some(SystemPermissionState::Allowed),
        Err(NotificationError::Denied) => Some(SystemPermissionState::Denied),
        Err(NotificationError::Failed(_)) => None,
    }
}

/// What a window reported of `view`, told to the view.
fn report_to(view: &ProgramView, report: ViewReport) {
    match report {
        ViewReport::Pointer { event, held_keys } => {
            view.pointer(event, &held_keys.into_iter().collect::<HeldModifiers>());
        }
        ViewReport::Focus(event) => view.focus(event),
        ViewReport::TextInputEditingValue(value) => view.text_input_editing_value(value),
        ViewReport::TextInputAction(action) => view.text_input_action(action),
        ViewReport::TextInputClosed => view.text_input_closed(),
        ViewReport::Copy => view.copy(),
        ViewReport::Cut => view.cut(),
        ViewReport::Paste => view.paste(),
        ViewReport::Edit(command) => view.edit(command),
        ViewReport::CloseRequested => view.close_requested(),
        ViewReport::PopRoute => view.pop_route(),
        ViewReport::MetricsChanged(metrics) => view.metrics_changed(metrics),
        ViewReport::ScreenChanged(screen) => view.screen_changed(screen),
        ViewReport::PresenceChanged(presence) => view.presence_changed(presence),
        ViewReport::WindowFrameChanged(frame) => view.window_frame_changed(frame),
        ViewReport::WindowMaximizedChanged(maximized) => view.window_maximized_changed(maximized),
        ViewReport::WindowFullscreenChanged(fullscreen) => {
            view.window_fullscreen_changed(fullscreen);
        }
        ViewReport::TitleBarDoubleClickActionChanged(action) => {
            view.title_bar_double_click_action_changed(action);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
