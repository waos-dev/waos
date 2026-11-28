//! At most one window process per running app, as Chrome's `AppShimManager` keeps a shim per app:
//! the processes the apps' windows, menus, dialogs, notifications and WaOS's prompts for them are
//! shown in, launched from the apps' bundles, and the listener they connect to (`server`). What
//! WaOS shows for an app reaches its window process through one getter ([`WindowProcesses::of`]),
//! which launches it the first time something needs it, with no window and not brought forward,
//! shares that launch with everything that asks while it comes, and answers why if it never comes;
//! and a second ([`WindowProcesses::running`]), which never launches, answering the window process
//! only while it runs. The program's windows, its file dialogs (`file_dialogs`), its notifications
//! (`notifications`) and WaOS's prompts for it (`state::prompt_dialogs`) take the first; its
//! context menus, which open at a view the window process already shows, and its menu bar, which
//! is sent as the window process connects, take the second. What the user must see brings the
//! window process forward as it shows, by macOS's cooperative activation: WaOS yields its
//! activation as it asks for a window that takes the focus, or a file dialog, so a launch for
//! anything else stays in the background.
//!
//! A launch that fails, or a window process that ends before its hello, is launched once more, its
//! bundle written again, as Chrome does; a second failure fails everything that waited. A window
//! process that ends after its hello, while its app's code runs, was crashed or force quit:
//! everything it owed is answered and the code ends with it, as nothing could show it any more;
//! and a window process ends when its app's code ends. A window process the user opened, from the
//! Dock or the Finder, or that macOS launched for a click on one of its app's notifications, says
//! so in its hello: it goes up as the user's open ([`UserOpen`]), and is served as the run that
//! opening answers.
//!
//! [`WindowProcesses`] is what macOS's parts of the engine's shell hold, and the one owner of the
//! runtime the launches and endings are carried out on and of the listener. What its threads and
//! tasks report to, each app's window process launching or connected, is its process table
//! ([`ProcessTable`]), which they share with it; as the last holder lets go, the listener stops,
//! the table is closed, every connected window process hearing its connection end, and the
//! runtime is shut down.
//!
//! The invariant a later editor will break: an app has one window process at a time, launched
//! once however many ask while it comes, and everything that asks is answered exactly once, in the
//! order it asked: with the window process, or with why it never came. Nothing a thread or a task
//! holds holds `WindowProcesses` itself, or the listener would never stop.

use std::collections::HashMap;
use std::future::ready;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Context, Result};
use futures::FutureExt;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::views::{ViewError, ViewId};
use waos::{
    MenuChosenFuture, RunningProgram, ShellContextMenus, ShellMenuBar, ViewPlacement, ViewRequest,
};
use waos_package::ProgramId;
use waos_window_messages::os::macos::AuditToken;

use super::canvases::CanvasesOnRequest;
use super::server::Listener;
use super::window_process::{DialogReports, SendToWindow, WindowPresences, WindowProcess};
use crate::logic::notification_permissions::NotificationPermissions;
use crate::os::yield_activation_to;
use crate::system_shell::UserOpen;

/// What reaching an app's window process answers: the window process, or why it never came.
pub(crate) type ReachedApp = Result<Arc<WindowProcess>, String>;

/// An app's window process, once it is there, for a route that awaits it.
pub(crate) type AppFuture = Pin<Box<dyn Future<Output = ReachedApp> + Send>>;

/// The app bundles window processes are launched from and checked against: `AppBundles`, or a
/// test's stand-in.
pub(crate) trait Bundles: Send + Sync {
    /// Starts `app`'s window process from its bundle through Launch Services, not brought
    /// forward, the bundle written first where it has none, or written again where `rewrite`
    /// asks; answers its process once it started.
    fn launch(
        self: Arc<Self>,
        app: ProgramId,
        rewrite: bool,
    ) -> BoxFuture<'static, Result<LaunchedProcess, LaunchFailure>>;

    /// Whether the process that sent `sender`'s message is `app`'s bundle as WaOS signed it; why
    /// not.
    fn check(self: Arc<Self>, app: &ProgramId, sender: &AuditToken) -> Result<(), String>;

    /// `program`'s name as its user knows it, which its window process shows.
    fn name(&self, program: &RunningProgram) -> String;
}

/// A window process Launch Services started: its process, and when it ends.
pub(crate) struct LaunchedProcess {
    pub(crate) pid: i32,
    /// Answers once the process has ended.
    pub(crate) exited: oneshot::Receiver<()>,
}

/// Why a window process did not start.
#[derive(Debug)]
pub(crate) enum LaunchFailure {
    /// The app has no bundle to launch it from, nor can it have one: not launched again.
    Bundle(String),
    /// Launch Services did not start it: launched once more, its bundle written again.
    LaunchServices(String),
}

/// Which launch of an app's window process this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchAttempt {
    First,
    /// The one after a launch that failed, whose bundle is written again.
    Again,
}

/// Where what a window process reports goes, each handed to every window process WaOS serves.
pub(crate) struct Routes {
    /// Where the canvases of the apps' views are kept, on the main thread.
    pub(crate) canvases: Arc<CanvasesOnRequest>,
    /// Where what the dialogs of WaOS's prompts report goes, on the main thread.
    pub(crate) dialog_reports: DialogReports,
    /// Where what the window processes say macOS said of the apps' notifications is kept.
    pub(crate) notification_permissions: NotificationPermissions,
    /// Where whether each of the apps' windows is in sight goes, on the main thread.
    pub(crate) window_presences: WindowPresences,
}

/// At most one window process per running app, and the listener they connect to.
pub(crate) struct WindowProcesses {
    table: Arc<ProcessTable>,
    listener: Listener,
    /// What the launches and endings are carried out on; taken as the window processes go.
    runtime: Option<tokio::runtime::Runtime>,
}

impl WindowProcesses {
    /// The window processes of the apps in `bundles`, launched and ended on `runtime`, what each
    /// reports going by `routes`, and listening for their hellos under the bootstrap name `name`
    /// from now on; the apps the user opens go to `user_opens`.
    pub(crate) fn start(
        runtime: tokio::runtime::Runtime,
        bundles: Arc<dyn Bundles>,
        routes: Routes,
        name: &str,
        user_opens: UnboundedSender<UserOpen>,
    ) -> Result<Arc<WindowProcesses>> {
        let table = Arc::new(ProcessTable {
            bundles: Arc::clone(&bundles),
            runtime: runtime.handle().clone(),
            routes,
            apps: Mutex::default(),
        });
        let listener = Listener::listen(name, Arc::clone(&table), bundles, user_opens)
            .context("listening for the apps' window processes")?;
        Ok(Arc::new(WindowProcesses {
            table,
            listener,
            runtime: Some(runtime),
        }))
    }

    /// `program`'s window process, once it is there: at once while it runs; otherwise launched
    /// with no window and not brought forward the first time something asks, everything that
    /// asks while it comes answered in the order it asked; or why it never came.
    pub(crate) fn of(&self, program: &RunningProgram) -> AppFuture {
        let (answer, answered) = oneshot::channel();
        self.table.with_app(program, move |app| {
            let _ = answer.send(app);
        });
        Box::pin(answered.map(|app| {
            app.unwrap_or_else(|_| Err("WaOS let go of the app's window process".to_owned()))
        }))
    }

    /// Hands `program`'s window process to `then`, as [`of`](Self::of) reaches it, on the thread
    /// that answers: this one where it runs.
    pub(crate) fn with_app(
        &self,
        program: &RunningProgram,
        then: impl FnOnce(ReachedApp) + Send + 'static,
    ) {
        self.table.with_app(program, then);
    }

    /// `program`'s window process while it runs, once it said hello; none before, or after it
    /// went. Never launches it.
    pub(crate) fn running(&self, program: &RunningProgram) -> Option<Arc<WindowProcess>> {
        self.table.running(program)
    }

    /// The process table, which a test tells of a window process said hello.
    #[cfg(test)]
    pub(crate) fn table(&self) -> &ProcessTable {
        &self.table
    }
}

/// The last holder let go: the listener stops, every connected window process hears its
/// connection end, and what still waits for one hears WaOS let go of it.
impl Drop for WindowProcesses {
    fn drop(&mut self) {
        self.listener.stop();
        self.table.close();
        if let Some(runtime) = self.runtime.take() {
            // The last holder may let go on one of the runtime's own workers, through an app's
            // code holding the engine's shell: a runtime cannot be waited for from inside itself.
            runtime.shutdown_background();
        }
    }
}

/// What reaching an app's window process waits with: the answer once it comes.
type Waiting = Box<dyn FnOnce(ReachedApp) + Send>;

/// Why something that waited for an app's window process hears it went to another run of the app.
const ANOTHER_RUN: &str = "another run of the program took its app";

/// Each app's window process as WaOS has it, launching or connected, and what waits for it; shared
/// by `WindowProcesses` with the threads and tasks that serve the window processes.
pub(crate) struct ProcessTable {
    bundles: Arc<dyn Bundles>,
    /// Where launches and endings are awaited, and a key's reply.
    runtime: tokio::runtime::Handle,
    routes: Routes,
    apps: Mutex<HashMap<ProgramId, App>>,
}

/// An app's window process, as WaOS has it.
enum App {
    /// WaOS launched it for `program`; its hello has not come. What asked for it meanwhile waits,
    /// in the order it asked.
    Launching {
        program: RunningProgram,
        waiting: Vec<Waiting>,
        /// Its process, once Launch Services started it.
        pid: Option<i32>,
        attempt: LaunchAttempt,
    },
    /// It said hello: it serves the program.
    Connected(Arc<WindowProcess>),
}

impl ProcessTable {
    /// Hands `program`'s window process to `then`: at once while it runs; otherwise once it said
    /// hello, launched the first time something asks, everything that asks while it comes handed
    /// it in the order it asked; or why it never came.
    fn with_app(
        self: &Arc<Self>,
        program: &RunningProgram,
        then: impl FnOnce(ReachedApp) + Send + 'static,
    ) {
        let id = match app_id(program) {
            Ok(id) => id,
            Err(why) => return then(Err(why)),
        };
        let mut apps = self.apps();
        match apps.get_mut(&id) {
            Some(App::Connected(process)) if process.program() == program => {
                let process = Arc::clone(process);
                drop(apps);
                return then(Ok(process));
            }
            Some(App::Launching {
                program: launching,
                waiting,
                ..
            }) if launching == program => return waiting.push(Box::new(then)),
            _ => {}
        }
        let launching = App::Launching {
            program: program.clone(),
            waiting: vec![Box::new(then)],
            pid: None,
            attempt: LaunchAttempt::First,
        };
        let displaced = apps.insert(id.clone(), launching);
        drop(apps);
        if let Some(App::Launching { waiting, .. }) = displaced {
            answer(waiting, &Err(ANOTHER_RUN.to_owned()));
        }
        self.launch(id, LaunchAttempt::First);
    }

    /// `program`'s window process while it runs, once it said hello.
    fn running(&self, program: &RunningProgram) -> Option<Arc<WindowProcess>> {
        let id = app_id(program).ok()?;
        match self.apps().get(&id)? {
            App::Connected(process) if process.program() == program => Some(Arc::clone(process)),
            _ => None,
        }
    }

    /// The program `id`'s window process is launched for, while it launches.
    pub(super) fn launching(&self, id: &ProgramId) -> Option<RunningProgram> {
        match self.apps().get(id)? {
            App::Launching { program, .. } => Some(program.clone()),
            App::Connected(_) => None,
        }
    }

    /// Launches `id`'s window process from its bundle, written again for `attempt`'s sake after a
    /// launch that failed; what comes of it is told to the table.
    fn launch(self: &Arc<Self>, id: ProgramId, attempt: LaunchAttempt) {
        let launching =
            Arc::clone(&self.bundles).launch(id.clone(), attempt == LaunchAttempt::Again);
        let table = Arc::clone(self);
        self.runtime.spawn(async move {
            match launching.await {
                Ok(launched) => table.launched(&id, launched),
                Err(LaunchFailure::Bundle(reason)) => table.give_up(&id, &reason),
                Err(LaunchFailure::LaunchServices(reason)) => table.launch_failed(&id, &reason),
            }
        });
    }

    /// `id`'s window process started as `launched` says: an end before its hello is a failed
    /// launch.
    fn launched(self: &Arc<Self>, id: &ProgramId, launched: LaunchedProcess) {
        let LaunchedProcess { pid, exited } = launched;
        if let Some(App::Launching { pid: started, .. }) = self.apps().get_mut(id) {
            *started = Some(pid);
        }
        let (table, id) = (Arc::clone(self), id.clone());
        self.runtime.spawn(async move {
            if exited.await.is_ok() {
                table.ended(&id, pid);
            }
        });
    }

    /// The process `pid` ended: if it was `id`'s window process, launched and not yet connected,
    /// its launch failed.
    fn ended(self: &Arc<Self>, id: &ProgramId, pid: i32) {
        let launching = matches!(
            self.apps().get(id),
            Some(App::Launching { pid: Some(started), .. }) if *started == pid
        );
        if launching {
            self.launch_failed(id, "its app ended before it said hello");
        }
    }

    /// The launch of `id`'s window process failed, for `reason`: after a first failure it is
    /// launched once more, its bundle written again; after a second, everything that waited fails.
    fn launch_failed(self: &Arc<Self>, id: &ProgramId, reason: &str) {
        tracing::warn!(program = %id, reason, "the program's app did not start");
        let again = match self.apps().get_mut(id) {
            Some(App::Launching {
                attempt: attempt @ LaunchAttempt::First,
                pid,
                ..
            }) => {
                *attempt = LaunchAttempt::Again;
                *pid = None;
                true
            }
            _ => false,
        };
        if again {
            self.launch(id.clone(), LaunchAttempt::Again);
        } else {
            self.give_up(id, reason);
        }
    }

    /// `id`'s window process will not come, for `reason`: everything that waited for it hears why.
    fn give_up(&self, id: &ProgramId, reason: &str) {
        let Some(App::Launching { waiting, .. }) = self.take_launching(id) else {
            return;
        };
        answer(
            waiting,
            &Err(format!("the program's app did not start: {reason}")),
        );
    }

    /// `program`'s window process said hello, and `send` reaches it: it serves the program, is
    /// handed to what waited for it, and ends when the program does.
    pub(super) fn connect(
        self: &Arc<Self>,
        id: &ProgramId,
        program: RunningProgram,
        send: SendToWindow,
    ) -> Arc<WindowProcess> {
        let routes = &self.routes;
        let process = WindowProcess::new(
            program,
            send,
            Arc::clone(&routes.canvases),
            routes.dialog_reports.clone(),
            routes.notification_permissions.clone(),
            routes.window_presences.clone(),
            self.runtime.clone(),
        );
        self.connected(id, &process);
        self.end_with_its_program(id.clone(), Arc::clone(&process));
        process
    }

    /// `process`, `id`'s window process, said hello: it shows the program's menu bar, and
    /// everything that waited for it gets it, in the order it asked.
    pub(crate) fn connected(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
        let before = self
            .apps()
            .insert(id.clone(), App::Connected(Arc::clone(process)));
        process.show_menu_bar();
        if let Some(App::Launching {
            program, waiting, ..
        }) = before
        {
            let reached = if &program == process.program() {
                Ok(Arc::clone(process))
            } else {
                Err(ANOTHER_RUN.to_owned())
            };
            answer(waiting, &reached);
        }
    }

    /// When `process`'s program ends, the window process closes its windows and ends, and `id` has
    /// none until something asks again.
    fn end_with_its_program(self: &Arc<Self>, id: ProgramId, process: Arc<WindowProcess>) {
        let (table, ended) = (Arc::clone(self), process.program().ended());
        self.runtime.spawn(async move {
            ended.await;
            process.program_ended();
            table.forget(&id, &process);
        });
    }

    /// `process`'s window process is gone: every view shown there goes with it, and so does the
    /// program, which nothing could show any more. WaOS ends a window process only once its
    /// program has ended, so one gone first was ended by the system or the user: a crash, or Force
    /// Quit.
    pub(super) fn disconnected(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
        tracing::info!(program = %id, "the program's app is gone: the program ends with it");
        process.gone();
        self.forget(id, process);
        process.program().stop();
    }

    /// `process`, `id`'s window process, is gone, or its program ended: `id` has none until
    /// something asks again.
    fn forget(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
        let mut apps = self.apps();
        if matches!(apps.get(id), Some(App::Connected(connected)) if Arc::ptr_eq(connected, process))
        {
            apps.remove(id);
        }
    }

    /// WaOS serves no window process any more: each connected one hears its connection end, and
    /// what waited for one hears WaOS let go of it.
    fn close(&self) {
        let apps = std::mem::take(&mut *self.apps());
        for app in apps.into_values() {
            if let App::Connected(process) = app {
                process.close();
            }
        }
    }

    fn take_launching(&self, id: &ProgramId) -> Option<App> {
        let mut apps = self.apps();
        match apps.get(id)? {
            App::Launching { .. } => apps.remove(id),
            App::Connected(_) => None,
        }
    }

    fn apps(&self) -> MutexGuard<'_, HashMap<ProgramId, App>> {
        self.apps.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The engine's placement on macOS: an app's windows open in its window process, a window that
/// takes the focus bringing it forward as it opens.
impl ViewPlacement for WindowProcesses {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        if request.config.visible && request.config.activating {
            let_come_forward(program);
        }
        let asking = program.clone();
        self.with_app(program, move |app| match app {
            Ok(app) => app.open_window(request),
            Err(why) => asking.view_request_failed(request.id, ViewError::Failed(why)),
        });
        Ok(())
    }
}

/// The engine's menu bar on macOS: an app's bar is its window process's, shown there as the engine
/// keeps it, the `entries` the app just set, while the window process runs; it is shown the bar as
/// it connects.
impl ShellMenuBar for WindowProcesses {
    fn set(&self, program: &RunningProgram, _entries: &[MenuEntry]) {
        if let Some(app) = self.running(program) {
            app.show_menu_bar();
        }
    }
}

/// The engine's context menus on macOS: an app's open in its window process, at a point of a view
/// shown there; one asked for while its window process does not run is answered none.
impl ShellContextMenus for WindowProcesses {
    fn show(
        &self,
        program: &RunningProgram,
        view: ViewId,
        x: f64,
        y: f64,
        entries: &[MenuEntry],
    ) -> MenuChosenFuture {
        match self.running(program) {
            Some(app) => app.show_context_menu(view, (x, y), entries),
            None => Box::pin(ready(None)),
        }
    }
}

/// The id of `program`'s app; why it has none.
pub(super) fn app_id(program: &RunningProgram) -> Result<ProgramId, String> {
    program
        .name()
        .parse()
        .map_err(|_| "a program with no id has no app".to_owned())
}

/// Lets `program`'s window process take WaOS's activation as what the user must see shows there.
pub(super) fn let_come_forward(program: &RunningProgram) {
    if let Ok(id) = app_id(program) {
        yield_activation_to(&id);
    }
}

/// Hands every one `waiting` what reaching the window process answered, in the order it asked.
fn answer(waiting: Vec<Waiting>, reached: &ReachedApp) {
    for then in waiting {
        then(reached.clone());
    }
}

#[cfg(test)]
pub(crate) mod tests;
