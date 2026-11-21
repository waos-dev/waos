//! One program's window process, as WaOS serves it once it said hello: each window the program
//! asks for is opened there, once WaOS made the canvas its view shows (`canvases`), with where
//! that canvas's layers are; each view shown there is added to the program, its hardware buffers
//! presented to the canvas, in WaOS, and its calls handed back as `ViewCall`s; what the window
//! reports of a view goes to the view, a new size placing the canvas at it, at once, or, where the
//! window waits in the resize, with the program's picture at that size under the window's fence,
//! the window then hearing the picture is shown (`canvas_pictures`); a key's reply goes back once
//! the program gives it; and when a window goes, its view and its canvas go with it, as every one
//! does when the window process goes. The program's menu bar
//! is shown there, the app's while it is in front, sent whole each time the program sets it and
//! as the window process connects, and a choice in it goes back to the program by its id; its
//! context menus open there at a point of a view shown there, each answering the program's call
//! once it closes, or with none once the window process goes (`awaited_answers`, decision 10 of
//! `docs/proposals/2026-10-29-macos-apps.md`). WaOS's prompts for the program are shown in dialogs
//! there, and what a dialog reports goes on to the main thread, where WaOS's view of the prompt is
//! ([`DialogReport`]); when the window process goes, every dialog it showed is reported closed.
//! The user's quitting the program's app asks the program, as WaOS's own Quit does, and the app
//! hears its answer; a program that agrees is stopped, and its app ends with it (decision 12). Its
//! file dialogs open there, a sheet on the window showing the view the program names, or on their
//! own where it names none or no window there shows it, each answering the program's call with the
//! paths the user picked once it closes, which the engine mounts, or with why none were once the
//! window process goes (decision 13). Its notifications are posted, scheduled and removed there,
//! under the app's bundle, each post answering the program's call once the system has it, or with
//! why not once the window process goes, and what the user does to one there goes to the program
//! (decision 14). macOS's own question for them is put there too, answering whether macOS refuses
//! the program its notifications, or nothing once the window process goes; macOS's permission for
//! them is read there again when WaOS asks; and whatever the app says macOS said of them, as WaOS
//! welcomed it, as the question is answered, as it comes to the front, as WaOS asks and as a post
//! is answered, goes to the program, whose permission reads it, and on to the main thread, where
//! WaOS's settings page for the program says so ([`SystemPermission`]).
//!
//! The invariant a later editor will break: nothing here waits on the window process or on the
//! main thread. A message is sent and the call returns, a canvas is made, placed and let go by a
//! call to the main thread, a dialog's report and what macOS said are sent on to it, a key's reply
//! is sent from the runtime the window process is served on, once the program answers, as the
//! answer to a quit is, and the answer to a context menu, a file dialog, a notification's post or
//! macOS's question resolves the program's call when it arrives.

use std::collections::{HashMap, HashSet};
use std::future::ready;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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
    PlacedView, PlacedWindow, ProgramView, RunningProgram, SystemPermissionFuture, ViewRequest,
};
use waos_package::ProgramId;
use waos_program_window::{CanvasLayer, CanvasView, ViewCalls, WindowCalls};
use waos_window_messages::os::macos::SendRight;
use waos_window_messages::{
    FileDialog, FileDialogId, FromWindow, HostedLayers, KeyId, OpenedWindow, PromptCall, PromptId,
    PromptReport, ToWindow, ViewReport,
};

use super::app_reach::app_id;
use super::awaited_answers::{APP_GONE, AppGone, AwaitedAnswer, AwaitedAnswers};
use super::canvas_pictures::CanvasPictures;
use super::canvases::{CanvasKey, CanvasesOnRequest};
use super::file_dialogs::failed;

/// What sends a message to the window process.
pub(crate) type SendToWindow = Arc<dyn Fn(ToWindow) + Send + Sync>;

/// What the dialog of one of WaOS's prompts reported, on its way to the main thread.
pub(crate) struct DialogReport {
    pub(crate) prompt: PromptId,
    pub(crate) report: PromptReport,
}

/// Where the dialogs' reports go, on their way to the main thread.
pub(crate) type DialogReports = UnboundedSender<DialogReport>;

/// What macOS said of a program's notifications, as its app told it: that it refuses them, or lets
/// them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SystemPermission {
    pub(crate) program: ProgramId,
    pub(crate) refuses: bool,
}

/// Where what macOS says of the programs' notifications goes, on its way to the main thread.
pub(crate) type SystemPermissions = UnboundedSender<SystemPermission>;

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
    program: RunningProgram,
    send: SendToWindow,
    canvases: Arc<CanvasesOnRequest>,
    /// Where a key's reply is awaited.
    runtime: tokio::runtime::Handle,
    /// Each window asked of the window process, by the request it was made for.
    windows: Mutex<HashMap<RequestId, AppWindow>>,
    /// Whether the window process is gone, after which a canvas made is let go at once and a
    /// prompt shown is closed at once. Set while the prompts are held, so none is shown after.
    gone: AtomicBool,
    dialog_reports: DialogReports,
    system_permissions: SystemPermissions,
    /// Whether each of the program's windows is in sight, on its way to the main thread.
    window_presences: WindowPresences,
    /// The prompts shown in dialogs there, until hidden or closed.
    prompts: Mutex<HashSet<PromptId>>,
    /// Held while the program's menu bar is read and sent, so the bar sent last is the one it set
    /// last.
    menu_bar_sending: Mutex<()>,
    /// Held while WaOS asks the app for something the program awaits, letting every resize waiting
    /// go first, and while it takes a new size a window waits at, so no resize is kept waiting
    /// behind such a request (`canvas_pictures`).
    asking: Mutex<()>,
    /// The context menus open there, each answering the item chosen in it.
    context_menus: AwaitedAnswers<Option<u32>>,
    /// The file dialogs open there, each answering the paths picked in it, or why none were.
    file_dialogs: AwaitedAnswers<Result<Vec<PathBuf>, DialogError>>,
    /// The notifications being posted or scheduled there, each answering whether the system has
    /// it.
    notifications: AwaitedAnswers<Result<(), NotificationError>>,
    /// macOS's questions for the program's notifications being put there, each answering whether
    /// macOS refuses them.
    notification_permissions: AwaitedAnswers<bool>,
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
    /// `program`'s window process, which `send` reaches, its views' canvases kept by `canvases`,
    /// what its prompts' dialogs report sent to `dialog_reports`, what it says macOS said of the
    /// program's notifications to `system_permissions`, and whether each of its windows is in
    /// sight to `window_presences`; key replies are awaited on `runtime`.
    pub(crate) fn new(
        program: RunningProgram,
        send: SendToWindow,
        canvases: Arc<CanvasesOnRequest>,
        dialog_reports: DialogReports,
        system_permissions: SystemPermissions,
        window_presences: WindowPresences,
        runtime: tokio::runtime::Handle,
    ) -> Arc<WindowProcess> {
        Arc::new(WindowProcess {
            program,
            send,
            canvases,
            runtime,
            windows: Mutex::default(),
            gone: AtomicBool::new(false),
            dialog_reports,
            system_permissions,
            window_presences,
            prompts: Mutex::default(),
            menu_bar_sending: Mutex::default(),
            asking: Mutex::default(),
            context_menus: AwaitedAnswers::default(),
            file_dialogs: AwaitedAnswers::default(),
            notifications: AwaitedAnswers::default(),
            notification_permissions: AwaitedAnswers::default(),
        })
    }

    /// Shows WaOS's prompt `prompt` in a dialog of the program's app, `size` logical points large,
    /// showing the layer context `layers`; closed at once once the window process is gone.
    pub(crate) fn show_prompt(&self, prompt: PromptId, size: [f64; 2], layers: HostedLayers) {
        let _asking = self.asking_the_app();
        let mut prompts = self.prompts();
        if self.gone.load(Ordering::SeqCst) {
            drop(prompts);
            self.dialog_reported(prompt, PromptReport::Closed);
            return;
        }
        prompts.insert(prompt);
        (self.send)(ToWindow::ShowPrompt {
            prompt,
            size,
            layers,
        });
    }

    /// WaOS's view of `prompt` asks its dialog for something.
    pub(crate) fn call_prompt(&self, prompt: PromptId, call: PromptCall) {
        (self.send)(ToWindow::Prompt { prompt, call });
    }

    /// Takes the dialog of `prompt` away, if it is still shown.
    pub(crate) fn hide_prompt(&self, prompt: PromptId) {
        if self.prompts().remove(&prompt) {
            (self.send)(ToWindow::HidePrompt { prompt });
        }
    }

    pub(crate) fn program(&self) -> &RunningProgram {
        &self.program
    }

    /// Shows the program's menu bar in its app as the engine keeps it, the one the program set
    /// last; nothing while it has set none, and the app shows the standard one.
    pub(crate) fn show_menu_bar(&self) {
        let _sending = self
            .menu_bar_sending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(bar) = self.program.menu_bar() {
            (self.send)(ToWindow::SetMenuBar(bar.to_vec()));
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
        let Some((menu, answered)) = self.context_menus.ask() else {
            return Box::pin(ready(None));
        };
        (self.send)(ToWindow::ShowContextMenu {
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
        let Some((dialog, picked)) = self.file_dialogs.ask() else {
            return Box::pin(ready(Err(failed(APP_GONE))));
        };
        (self.send)(ToWindow::OpenFileDialog {
            dialog,
            parent: parent.and_then(|view| self.window_showing(view)),
            kind,
        });
        Box::pin(WaitFileDialog {
            dialog,
            send: Arc::clone(&self.send),
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
        let Some((post, posted)) = self.notifications.ask() else {
            return Box::pin(ready(Err(app_gone())));
        };
        (self.send)(ToWindow::PostNotification {
            post,
            notification,
            at,
        });
        Box::pin(posted.map(move |posted| posted.unwrap_or_else(|AppGone| Err(app_gone()))))
    }

    /// Takes the program's notification `id` away in the app, shown or scheduled.
    pub(crate) fn remove_notification(&self, id: String) {
        (self.send)(ToWindow::RemoveNotification { id });
    }

    /// Puts macOS's own question for the program's notifications in the app, which asks the user
    /// only if they were never asked, and answers whether macOS refuses them once it has answered;
    /// nothing once the window process is gone.
    pub(crate) fn request_notification_permission(&self) -> SystemPermissionFuture {
        let _asking = self.asking_the_app();
        let Some((request, answered)) = self.notification_permissions.ask() else {
            return Box::pin(ready(None));
        };
        (self.send)(ToWindow::RequestNotificationPermission { request });
        Box::pin(answered.map(Result::ok))
    }

    /// Has the app read macOS's permission for the program's notifications again, without asking
    /// the user; it tells what it read as it tells every reading.
    pub(crate) fn read_notification_permission(&self) {
        (self.send)(ToWindow::ReadNotificationPermission);
    }

    /// The program asked for a window as `request` says: WaOS makes its canvas, then asks the
    /// window process for the window ([`canvas_made`](Self::canvas_made)).
    pub(crate) fn open_window(self: &Arc<Self>, request: ViewRequest) {
        self.canvases.make(Arc::clone(self), request);
    }

    /// The canvas of the window `request` asks for is made, under `key`, its layers where `layers`
    /// says: the window process makes the window.
    pub(crate) fn canvas_made(
        &self,
        key: CanvasKey,
        request: ViewRequest,
        canvas: Arc<CanvasLayer>,
        layers: HostedLayers,
    ) {
        if self.gone.load(Ordering::SeqCst) {
            self.canvases.remove(key);
            return;
        }
        let pictures = CanvasPictures::new(
            request.id,
            canvas,
            Arc::clone(&self.canvases),
            key,
            Arc::clone(&self.send),
        );
        let window = AppWindow {
            pictures,
            key,
            view: None,
            in_sight: false,
        };
        let _asking = self.asking_the_app();
        self.windows().insert(request.id, window);
        (self.send)(ToWindow::OpenWindow {
            window: request.id,
            config: request.config,
            layers,
        });
    }

    /// No canvas could be made for the window `request` asks for, for `reason`: it fails.
    pub(crate) fn canvas_refused(&self, request: ViewRequest, reason: &str) {
        let error = ViewError::Failed(format!("WaOS could not keep the window's canvas: {reason}"));
        self.program.view_request_failed(request.id, error);
    }

    /// The program ended: the window process closes its windows and ends.
    pub(crate) fn program_ended(&self) {
        (self.send)(ToWindow::ProgramEnded);
    }

    /// Carries out what the window process sent, with the ports that came with it.
    pub(crate) fn received(&self, message: FromWindow, ports: Vec<SendRight>) {
        tracing::trace!(program = self.program.name(), ?message);
        match message {
            FromWindow::Hello(_) => {
                tracing::warn!(program = self.program.name(), "a second hello");
            }
            FromWindow::WindowOpened { window, opened } => self.add_view(window, opened),
            FromWindow::WindowRefused { window, error } => {
                self.forget_window(window);
                self.program.view_request_failed(window, error);
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
            FromWindow::Opened(request) => self.program.open_requested(request),
            FromWindow::Prompt { prompt, report } => self.dialog_reported(prompt, report),
            FromWindow::MenuBarChosen(id) => self.program.menu_chosen(id),
            FromWindow::ContextMenuClosed { menu, chosen } => {
                self.context_menus.answered(menu, chosen);
            }
            FromWindow::QuitRequested => self.quit_requested(),
            FromWindow::FileDialogClosed { dialog, picked } => {
                self.file_dialogs.answered(dialog, picked);
            }
            FromWindow::NotificationPosted { post, posted } => {
                if let Some(refused) = system_refuses(&posted) {
                    self.system_said(refused);
                }
                self.notifications.answered(post, posted);
            }
            FromWindow::NotificationActivated(activation) => {
                self.program.notification_activated(activation);
            }
            FromWindow::NotificationPermission { refused } => self.system_said(refused),
            FromWindow::NotificationPermissionAnswered { request, refused } => {
                self.system_said(refused);
                self.notification_permissions.answered(request, refused);
            }
        }
    }

    /// The app said whether macOS `refused` the program its notifications: the program's
    /// permission reads it, and WaOS's settings page for the program hears it.
    fn system_said(&self, refused: bool) {
        self.program.system_notifications_refused(refused);
        if let Ok(program) = app_id(&self.program) {
            let said = SystemPermission {
                program,
                refuses: refused,
            };
            let _ = self.system_permissions.unbounded_send(said);
        }
    }

    /// The user quit the program's app: the program is asked, as WaOS's own Quit asks it, and the
    /// app hears its answer; a program that agrees is stopped, and its app ends with it.
    fn quit_requested(&self) {
        let asked = self.program.request_exit();
        let (send, program) = (Arc::clone(&self.send), self.program.clone());
        self.runtime.spawn(async move {
            let response = asked.await;
            send(ToWindow::QuitAnswered(response));
            if response == AppExitResponse::Exit {
                program.stop();
            }
        });
    }

    /// The window process is gone: every view shown there is taken away, every canvas let go, every
    /// context menu answered none, every file dialog answered with why nothing was picked, every
    /// notification being posted answered with why it was not, every question of macOS's answered
    /// nothing, and every prompt's dialog closed.
    pub(crate) fn gone(&self) {
        let shown: Vec<PromptId> = {
            let mut prompts = self.prompts();
            self.gone.store(true, Ordering::SeqCst);
            prompts.drain().collect()
        };
        let windows: Vec<RequestId> = self.windows().keys().copied().collect();
        for window in windows {
            self.forget_window(window);
        }
        self.context_menus.app_gone();
        self.file_dialogs.app_gone();
        self.notifications.app_gone();
        self.notification_permissions.app_gone();
        for prompt in shown {
            self.dialog_reported(prompt, PromptReport::Closed);
        }
    }

    /// The dialog of `prompt` reported `report`, which goes on to the main thread; a dialog closed
    /// is no longer shown.
    fn dialog_reported(&self, prompt: PromptId, report: PromptReport) {
        if matches!(report, PromptReport::Closed) {
            self.prompts().remove(&prompt);
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
        let Some(pictures) = self.pictures_of(window) else {
            tracing::warn!(window, "a window opened that WaOS did not ask for");
            return;
        };
        pictures.resize_at_once(opened.metrics, None);
        let send = Arc::clone(&self.send);
        let calls = ViewCalls::new(move |call| send(ToWindow::View { window, call }));
        let view = self.program.add_view(
            window,
            PlacedView {
                view: Arc::new(CanvasView::new(pictures, calls.clone())),
                text_input: Some(Arc::new(calls.clone())),
                window: Some(PlacedWindow {
                    window: Arc::new(WindowCalls::new(calls, opened.size_limits)),
                    frame: opened.frame,
                    maximized: opened.maximized,
                    title_bar_double_click_action: opened.title_bar_double_click_action,
                }),
                metrics: opened.metrics,
                screen: opened.screen,
                presence: opened.presence,
            },
        );
        tracing::info!(
            program = self.program.name(),
            window,
            view = view.id(),
            "a window was opened in the program's app"
        );
        if let Some(shown) = self.windows().get_mut(&window) {
            shown.view = Some(view);
        }
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
                (None, _) if waits => (self.send)(ToWindow::PictureShown {
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

    /// Whether the program awaits the app's answer to something WaOS asked of it, which the app
    /// carries out on its main thread: a window asked for and not yet open, a prompt's dialog
    /// shown, a context menu, a file dialog, a notification's post or macOS's question for the
    /// program's notifications. The window process keeps the same list (its `resize_waits`).
    fn owes_answers(&self) -> bool {
        self.windows().values().any(|window| window.view.is_none())
            || !self.prompts().is_empty()
            || self.context_menus.any()
            || self.file_dialogs.any()
            || self.notifications.any()
            || self.notification_permissions.any()
    }

    /// WaOS is about to ask the app for something the program awaits: every resize a window waits
    /// in lets its fence go first, its canvas laid out at once, since the window's wait ends as
    /// the request arrives; held until it is asked.
    fn asking_the_app(&self) -> MutexGuard<'_, ()> {
        let asking = self.asking();
        let waiting: Vec<Arc<CanvasPictures>> = self
            .windows()
            .values()
            .map(|window| Arc::clone(&window.pictures))
            .collect();
        for pictures in waiting {
            pictures.let_resize_go();
        }
        asking
    }

    fn asking(&self) -> MutexGuard<'_, ()> {
        self.asking.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The window `window` is gone, or never came: its view is taken away, and its canvas, which
    /// nothing will show again, let go.
    fn forget_window(&self, window: RequestId) {
        let Some(gone) = self.windows().remove(&window) else {
            return;
        };
        if gone.in_sight {
            self.tell_in_sight(window, false);
        }
        gone.pictures.window_gone();
        self.canvases.remove(gone.key);
        if let Some(view) = gone.view {
            self.program.remove_view(view.id());
        }
    }

    /// Keeps whether the window `window` is in sight, telling the main thread when it changes.
    fn window_in_sight_changed(&self, window: RequestId, in_sight: bool) {
        let changed = self
            .windows()
            .get_mut(&window)
            .is_some_and(|shown| std::mem::replace(&mut shown.in_sight, in_sight) != in_sight);
        if changed {
            self.tell_in_sight(window, in_sight);
        }
    }

    /// Tells the main thread whether the window `window` is in sight now, for the activity that
    /// keeps macOS from throttling WaOS. A program with no id has no app, and no window here.
    fn tell_in_sight(&self, window: RequestId, in_sight: bool) {
        let Ok(program) = app_id(&self.program) else {
            return;
        };
        let _ = self.window_presences.unbounded_send(WindowPresence {
            program,
            window,
            in_sight,
        });
    }

    /// Hands the program the key; its reply goes back once it gives one.
    fn key(&self, window: RequestId, key: KeyId, event: KeyEvent) {
        let Some(view) = self.view(window) else {
            (self.send)(ToWindow::KeyReplied {
                key,
                handled: false,
            });
            return;
        };
        let handled = view.key(event);
        let send = Arc::clone(&self.send);
        self.runtime.spawn(async move {
            let handled = handled.await;
            send(ToWindow::KeyReplied { key, handled });
        });
    }

    fn view(&self, window: RequestId) -> Option<ProgramView> {
        self.windows().get(&window)?.view.clone()
    }

    /// The window that shows the program's view `view`, by the request it was made for.
    fn window_showing(&self, view: ViewId) -> Option<RequestId> {
        self.windows().iter().find_map(|(window, shown)| {
            let shows = shown.view.as_ref()?.id() == view;
            shows.then_some(*window)
        })
    }

    fn pictures_of(&self, window: RequestId) -> Option<Arc<CanvasPictures>> {
        Some(Arc::clone(&self.windows().get(&window)?.pictures))
    }

    fn windows(&self) -> MutexGuard<'_, HashMap<RequestId, AppWindow>> {
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn prompts(&self) -> MutexGuard<'_, HashSet<PromptId>> {
        self.prompts.lock().unwrap_or_else(PoisonError::into_inner)
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
            (self.send)(ToWindow::CloseFileDialog {
                dialog: self.dialog,
            });
        }
    }
}

/// What the app's answer `posted` says of macOS: `denied`, that it refuses the program's
/// notifications; taken, that it lets them; a failure, nothing.
fn system_refuses(posted: &Result<(), NotificationError>) -> Option<bool> {
    match posted {
        Ok(()) => Some(false),
        Err(NotificationError::Denied) => Some(true),
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
        ViewReport::TitleBarDoubleClickActionChanged(action) => {
            view.title_bar_double_click_action_changed(action);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
