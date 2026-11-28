//! At most one window process per app, as Chrome's `AppShimManager` keeps a shim per app: the
//! processes the apps' windows, menus, dialogs, notifications and WaOS's prompts for them are shown
//! in, launched from the apps' bundles by the app's id, and the listener they connect to
//! (`server`). A window process belongs to the app, and lasts as long as the app's instance runs,
//! as a macOS app's process does, its Dock icon and menu bar with it: the instance running, as the
//! watch of the instances running tells ([`RunningInstances`]), attaches to it as the instance
//! starts or as the window process is launched, once, so a window process serves at most one
//! instance in its life; one launched for a prompt while no instance runs is served with none.
//! What an instance shows reaches its window process through one getter
//! ([`WindowProcesses::of_instance`]), which launches it the first time something needs it, with no
//! window and not brought forward, shares that launch with everything that asks while it comes,
//! and answers why if it never comes; and a second ([`WindowProcesses::running`]), which never
//! launches, answering the window process only while it runs and serves that instance. The
//! instance's windows, its file dialogs (`file_dialogs`) and its notifications (`notifications`)
//! take the first; its context menus, which open at a view the window process already shows, and
//! its menu bar, which is sent as the window process connects, take the second. WaOS's prompts
//! for an app go by the app's id ([`WindowProcesses::show_prompt`]): shown in its window process,
//! launched for them where it does not run, and kept here until they are hidden, so a window
//! process launched while its app does not run ends once it shows no prompt, as Android's task of
//! a permission question goes once it is answered, unless the app started meanwhile: the app's
//! first window then opens in it. What the user must see brings the window process forward as
//! it shows, by macOS's cooperative activation: WaOS yields its activation as it asks for a window
//! that takes the focus, or a file dialog, so a launch for anything else stays in the background.
//!
//! A launch that fails, or a window process that ends before its hello, is launched once more, its
//! bundle written again, as Chrome does; a second failure fails everything that waited, and puts
//! its prompts away. A window process that ends after its hello was crashed or force quit:
//! everything it owed is answered and its instance, if one attached, ends with it, as nothing could
//! show it any more; and a window process ends when its instance ends. A window process the user
//! opened, from the Dock or the Finder, or that macOS launched for a click on one of its app's
//! notifications, says so in its hello: it goes up as the user's open ([`UserOpen`]), and is served
//! with the instance that opening answers attached.
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
//! order it asked: with the window process, or with why it never came. An instance attaches, and a
//! window process with none is ended where no instance of its app runs, under the table's one
//! lock, so no instance attaches to a window process that is ending, and none that runs is left
//! without its window process. Nothing a thread or a task holds holds `WindowProcesses` itself,
//! or the listener would never stop.

use std::collections::HashMap;
use std::future::ready;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Context, Result};
use futures::FutureExt;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use tokio::sync::watch;
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::views::{ViewError, ViewId};
use waos::{
    AppInstance, MenuChosenFuture, ShellContextMenus, ShellMenuBar, ViewPlacement, ViewRequest,
};
use waos_package::ProgramId;
use waos_window_messages::os::macos::AuditToken;
use waos_window_messages::{HostedLayers, PromptCall, PromptId, PromptReport};

use super::canvases::CanvasesOnRequest;
use super::server::Listener;
use super::window_process::{
    DialogReport, DialogReports, SendToWindow, WindowPresences, WindowProcess,
};
use crate::logic::app_instances::RunningInstances;
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

    /// `app`'s name as its user knows it, which its window process shows.
    fn name(&self, app: &ProgramId) -> String;
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

/// At most one window process per app, and the listener they connect to.
pub(crate) struct WindowProcesses {
    table: Arc<ProcessTable>,
    listener: Listener,
    /// What the launches and endings are carried out on; taken as the window processes go.
    runtime: Option<tokio::runtime::Runtime>,
}

impl WindowProcesses {
    /// The window processes of the apps in `bundles`, launched and ended on `runtime`, each
    /// lasting while its app's instance in `running` runs, what each reports going by `routes`,
    /// and listening for their hellos under the bootstrap name `name` from now on; the apps the
    /// user opens go to `user_opens`.
    pub(crate) fn start(
        runtime: tokio::runtime::Runtime,
        bundles: Arc<dyn Bundles>,
        running: watch::Receiver<RunningInstances>,
        routes: Routes,
        name: &str,
        user_opens: UnboundedSender<UserOpen>,
    ) -> Result<Arc<WindowProcesses>> {
        let table = Arc::new(ProcessTable {
            bundles: Arc::clone(&bundles),
            runtime: runtime.handle().clone(),
            routes,
            running: running.clone(),
            apps: Mutex::default(),
        });
        table.follow_running(running);
        let listener = Listener::listen(name, Arc::clone(&table), bundles, user_opens)
            .context("listening for the apps' window processes")?;
        Ok(Arc::new(WindowProcesses {
            table,
            listener,
            runtime: Some(runtime),
        }))
    }

    /// `program`'s window process, once it is there, with `program` attached to it as its app's
    /// instance: at once while it runs; otherwise launched with no window and not brought forward
    /// the first time something asks, everything that asks while it comes answered in the order it
    /// asked; or why it never came.
    pub(crate) fn of_instance(&self, program: &AppInstance) -> AppFuture {
        let (answer, answered) = oneshot::channel();
        self.table.with_instance(program, move |app| {
            let _ = answer.send(app);
        });
        Box::pin(answered.map(|app| {
            app.unwrap_or_else(|_| Err("WaOS let go of the app's window process".to_owned()))
        }))
    }

    /// Hands `program`'s window process to `then`, as [`of_instance`](Self::of_instance) reaches
    /// it, on the thread that answers: this one where it runs.
    pub(crate) fn with_instance(
        &self,
        program: &AppInstance,
        then: impl FnOnce(ReachedApp) + Send + 'static,
    ) {
        self.table.with_instance(program, then);
    }

    /// `program`'s window process while it runs and serves `program`, once it said hello; none
    /// before, or after it went. Never launches it.
    pub(crate) fn running(&self, program: &AppInstance) -> Option<Arc<WindowProcess>> {
        self.table.running(program)
    }

    /// Every app's window process that runs now, once it said hello.
    pub(crate) fn all_running(&self) -> Vec<Arc<WindowProcess>> {
        self.table.all_running()
    }

    /// Shows WaOS's prompt `prompt` in a dialog of `app`'s window process, `size` logical points
    /// large, showing the layer context `layers`: there at once while it runs, or once it is
    /// there, launched for it where it does not; reported closed where it never comes.
    pub(crate) fn show_prompt(
        &self,
        app: &ProgramId,
        prompt: PromptId,
        size: [f64; 2],
        layers: HostedLayers,
    ) {
        let shown = PromptShown {
            prompt,
            size,
            layers,
        };
        self.table.show_prompt(app, shown);
    }

    /// WaOS's view of `app`'s prompt `prompt` asks its dialog for something: nothing before the
    /// dialog is there.
    pub(crate) fn call_prompt(&self, app: &ProgramId, prompt: PromptId, call: PromptCall) {
        if let Some(process) = self.table.connected_process(app) {
            process.call_prompt(prompt, call);
        }
    }

    /// Takes the dialog of `app`'s prompt `prompt` away, or the prompt from those waiting for
    /// the app; a window process ends once it shows no prompt, unless its app runs.
    pub(crate) fn hide_prompt(&self, app: &ProgramId, prompt: PromptId) {
        self.table.hide_prompt(app, prompt);
    }

    /// The process table, which a test tells of a window process said hello.
    #[cfg(test)]
    pub(crate) fn table(&self) -> &Arc<ProcessTable> {
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

/// Why something that waited for an app's window process hears it went to another instance of the
/// app.
const ANOTHER_INSTANCE: &str = "another instance of the app took its window process";

/// Each app's window process as WaOS has it, launching or connected, and what waits for it; shared
/// by `WindowProcesses` with the threads and tasks that serve the window processes.
pub(crate) struct ProcessTable {
    bundles: Arc<dyn Bundles>,
    /// Where launches and endings are awaited, and a key's reply.
    runtime: tokio::runtime::Handle,
    routes: Routes,
    /// The instances running, each of which its app's window process serves.
    running: watch::Receiver<RunningInstances>,
    apps: Mutex<HashMap<ProgramId, App>>,
}

/// An app's window process, as WaOS has it.
enum App {
    /// WaOS launched it; its hello has not come.
    Launching(Launching),
    /// It said hello: it serves the app.
    Connected(Arc<WindowProcess>),
}

/// An app's window process WaOS launched, until its hello comes.
struct Launching {
    /// The app's instance that runs or reached it first, attached to it once it is there; none
    /// where it was launched for a prompt while its app does not run.
    instance: Option<AppInstance>,
    /// What asked for it meanwhile, in the order it asked.
    waiting: Vec<Waiting>,
    /// WaOS's prompts to show there once it is there, in the order asked.
    prompts: Vec<PromptShown>,
    /// Its process, once Launch Services started it.
    pid: Option<i32>,
    attempt: LaunchAttempt,
}

/// One of WaOS's prompts, as a window process shows it in a dialog.
struct PromptShown {
    prompt: PromptId,
    size: [f64; 2],
    layers: HostedLayers,
}

impl Launching {
    /// A first launch, for `instance` where an instance asked, waited for by `waiting`, showing
    /// `prompts` once it is there.
    fn first(
        instance: Option<AppInstance>,
        waiting: Vec<Waiting>,
        prompts: Vec<PromptShown>,
    ) -> App {
        App::Launching(Launching {
            instance,
            waiting,
            prompts,
            pid: None,
            attempt: LaunchAttempt::First,
        })
    }
}

impl ProcessTable {
    /// Hands `program`'s window process to `then`, `program` attached to it as its app's instance:
    /// at once while it runs; otherwise once it said hello, launched the first time something
    /// asks, everything that asks while it comes handed it in the order it asked; or why it never
    /// came. One serving another instance of the app, which ended and whose window process is
    /// ending with it, is left to end, and another is launched.
    fn with_instance(
        self: &Arc<Self>,
        program: &AppInstance,
        then: impl FnOnce(ReachedApp) + Send + 'static,
    ) {
        let id = match app_id(program) {
            Ok(id) => id,
            Err(why) => return then(Err(why)),
        };
        let mut apps = self.apps();
        let mut prompts = Vec::new();
        match apps.get_mut(&id) {
            Some(App::Connected(process)) if process.serves(program) => {
                let process = Arc::clone(process);
                drop(apps);
                return then(Ok(process));
            }
            Some(App::Connected(process)) if process.instance().is_none() => {
                let process = Arc::clone(process);
                self.attach(apps, &id, &process, program.clone());
                return then(Ok(process));
            }
            Some(App::Launching(launching))
                if launching
                    .instance
                    .as_ref()
                    .is_none_or(|first| first == program) =>
            {
                launching.instance = Some(program.clone());
                return launching.waiting.push(Box::new(then));
            }
            Some(App::Launching(launching)) => prompts = std::mem::take(&mut launching.prompts),
            _ => {}
        }
        let launching = Launching::first(Some(program.clone()), vec![Box::new(then)], prompts);
        let displaced = apps.insert(id.clone(), launching);
        drop(apps);
        if let Some(App::Launching(Launching { waiting, .. })) = displaced {
            answer(waiting, &Err(ANOTHER_INSTANCE.to_owned()));
        }
        self.launch(id, LaunchAttempt::First);
    }

    /// `program`'s window process while it runs and serves `program`, once it said hello.
    fn running(&self, program: &AppInstance) -> Option<Arc<WindowProcess>> {
        let id = app_id(program).ok()?;
        self.connected_process(&id)
            .filter(|process| process.serves(program))
    }

    /// `id`'s window process while it runs, once it said hello.
    fn connected_process(&self, id: &ProgramId) -> Option<Arc<WindowProcess>> {
        match self.apps().get(id)? {
            App::Connected(process) => Some(Arc::clone(process)),
            App::Launching(_) => None,
        }
    }

    /// Every window process connected now.
    fn all_running(&self) -> Vec<Arc<WindowProcess>> {
        (self.apps().values())
            .filter_map(|app| match app {
                App::Connected(process) => Some(Arc::clone(process)),
                App::Launching(_) => None,
            })
            .collect()
    }

    /// Whether WaOS launched `id`'s window process and waits for its hello.
    pub(super) fn launching(&self, id: &ProgramId) -> bool {
        matches!(self.apps().get(id), Some(App::Launching(_)))
    }

    /// Shows `shown` in `id`'s window process: at once while it runs, or once it is there,
    /// launched for it where it does not, for the app's instance where one runs.
    fn show_prompt(self: &Arc<Self>, id: &ProgramId, shown: PromptShown) {
        let mut apps = self.apps();
        match apps.get_mut(id) {
            Some(App::Connected(process)) => {
                process.show_prompt(shown.prompt, shown.size, shown.layers);
            }
            Some(App::Launching(launching)) => launching.prompts.push(shown),
            None => {
                let instance = self.instance_running(id);
                apps.insert(
                    id.clone(),
                    Launching::first(instance, Vec::new(), vec![shown]),
                );
                drop(apps);
                self.launch(id.clone(), LaunchAttempt::First);
            }
        }
    }

    /// Takes `id`'s prompt `prompt` away, shown or waiting for its window process; one connected
    /// with no prompt left is ended where its app does not run.
    fn hide_prompt(self: &Arc<Self>, id: &ProgramId, prompt: PromptId) {
        let mut apps = self.apps();
        match apps.get_mut(id) {
            Some(App::Connected(process)) => {
                let process = Arc::clone(process);
                process.hide_prompt(prompt);
                self.end_if_unneeded(apps, id, &process);
            }
            Some(App::Launching(launching)) => {
                launching.prompts.retain(|shown| shown.prompt != prompt);
            }
            None => {}
        }
    }

    /// Ends `process`, `id`'s window process, where it shows no prompt and its app does not run:
    /// nothing needs it any more. Where the app's instance runs and has not attached to it yet, it
    /// attaches now, and `process` ends with it. Decided under `apps`, the table's lock, which an
    /// instance attaches under.
    fn end_if_unneeded(
        self: &Arc<Self>,
        mut apps: MutexGuard<'_, HashMap<ProgramId, App>>,
        id: &ProgramId,
        process: &Arc<WindowProcess>,
    ) {
        if process.instance().is_some() || process.shows_prompts() {
            return;
        }
        if !matches!(apps.get(id), Some(App::Connected(connected)) if Arc::ptr_eq(connected, process))
        {
            return;
        }
        if let Some(instance) = self.instance_running(id) {
            return self.attach(apps, id, process, instance);
        }
        apps.remove(id);
        drop(apps);
        tracing::info!(app = %id, "the app's window process shows nothing more: it ends");
        process.end();
    }

    /// The instance of `id` running now, if one is.
    fn instance_running(&self, id: &ProgramId) -> Option<AppInstance> {
        self.running.borrow().get(id).cloned()
    }

    /// From now on, each instance that starts attaches to its app's window process, launching or
    /// connected, where there is one serving none, as [`attach_running`](Self::attach_running)
    /// does: until the instances running are no longer told.
    fn follow_running(self: &Arc<Self>, mut running: watch::Receiver<RunningInstances>) {
        let table = Arc::clone(self);
        self.runtime.spawn(async move {
            while running.changed().await.is_ok() {
                let ids: Vec<ProgramId> = running.borrow_and_update().keys().cloned().collect();
                for id in ids {
                    table.attach_running(&id);
                }
            }
        });
    }

    /// The instance of `id` running now, if one is, attaches to `id`'s window process, launching
    /// or connected, where it serves none yet: the window process lasts as long as that instance
    /// runs.
    fn attach_running(self: &Arc<Self>, id: &ProgramId) {
        let Some(instance) = self.instance_running(id) else {
            return;
        };
        let mut apps = self.apps();
        match apps.get_mut(id) {
            Some(App::Launching(launching)) if launching.instance.is_none() => {
                launching.instance = Some(instance);
            }
            Some(App::Connected(process)) if process.instance().is_none() => {
                let process = Arc::clone(process);
                self.attach(apps, id, &process, instance);
            }
            _ => {}
        }
    }

    /// `instance` attaches to `process`, `id`'s connected window process, which serves none yet,
    /// under `apps`, the table's lock, let go of after: `process` shows the instance's menu bar,
    /// and ends when `instance` does.
    fn attach(
        self: &Arc<Self>,
        apps: MutexGuard<'_, HashMap<ProgramId, App>>,
        id: &ProgramId,
        process: &Arc<WindowProcess>,
        instance: AppInstance,
    ) {
        process.attach(instance.clone());
        drop(apps);
        process.show_menu_bar();
        self.end_with_its_instance(id.clone(), Arc::clone(process), instance);
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
        if let Some(App::Launching(launching)) = self.apps().get_mut(id) {
            launching.pid = Some(pid);
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
            Some(App::Launching(Launching { pid: Some(started), .. })) if *started == pid
        );
        if launching {
            self.launch_failed(id, "its app ended before it said hello");
        }
    }

    /// The launch of `id`'s window process failed, for `reason`: after a first failure it is
    /// launched once more, its bundle written again; after a second, everything that waited fails.
    fn launch_failed(self: &Arc<Self>, id: &ProgramId, reason: &str) {
        tracing::warn!(app = %id, reason, "the app's window process did not start");
        let again = match self.apps().get_mut(id) {
            Some(App::Launching(launching)) if launching.attempt == LaunchAttempt::First => {
                launching.attempt = LaunchAttempt::Again;
                launching.pid = None;
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

    /// `id`'s window process will not come, for `reason`: everything that waited for it hears why,
    /// and each prompt that waited is reported closed.
    fn give_up(&self, id: &ProgramId, reason: &str) {
        let Some(Launching {
            waiting, prompts, ..
        }) = self.take_launching(id)
        else {
            return;
        };
        answer(
            waiting,
            &Err(format!("the app's window process did not start: {reason}")),
        );
        for shown in prompts {
            let closed = DialogReport {
                prompt: shown.prompt,
                report: PromptReport::Closed,
            };
            let _ = self.routes.dialog_reports.unbounded_send(closed);
        }
    }

    /// `id`'s window process said hello, and `send` reaches it, `instance` attached to it where
    /// the hello came of opening that instance: it serves the app, the instance that reached it
    /// while it came attached to it, is handed to what waited for it, and ends when its instance
    /// does.
    pub(super) fn connect(
        self: &Arc<Self>,
        id: &ProgramId,
        instance: Option<AppInstance>,
        send: SendToWindow,
    ) -> Arc<WindowProcess> {
        let routes = &self.routes;
        let process = WindowProcess::new(
            id.clone(),
            send,
            Arc::clone(&routes.canvases),
            routes.dialog_reports.clone(),
            routes.notification_permissions.clone(),
            routes.window_presences.clone(),
            self.runtime.clone(),
        );
        if let Some(instance) = instance {
            process.attach(instance);
        }
        self.connected(id, &process);
        process
    }

    /// `process`, `id`'s window process, said hello: the instance that reached it while it came
    /// is attached to it, unless another is already, and it ends when that instance does; it
    /// shows the instance's menu bar and the prompts that waited for it, and everything that
    /// waited for it gets it, in the order it asked, but what waited for another instance; and it
    /// ends at once where nothing needs it any more.
    pub(crate) fn connected(self: &Arc<Self>, id: &ProgramId, process: &Arc<WindowProcess>) {
        let mut apps = self.apps();
        let before = apps.insert(id.clone(), App::Connected(Arc::clone(process)));
        let Some(App::Launching(Launching {
            instance,
            waiting,
            prompts,
            ..
        })) = before
        else {
            drop(apps);
            self.served(id, process);
            return;
        };
        if let Some(instance) = &instance {
            process.attach(instance.clone());
        }
        for shown in prompts {
            process.show_prompt(shown.prompt, shown.size, shown.layers);
        }
        drop(apps);
        self.served(id, process);
        let reached = if instance.is_none_or(|asked| process.serves(&asked)) {
            Ok(Arc::clone(process))
        } else {
            Err(ANOTHER_INSTANCE.to_owned())
        };
        answer(waiting, &reached);
        self.end_if_unneeded(self.apps(), id, process);
    }

    /// `process`, `id`'s window process, is served from now on: it shows its instance's menu
    /// bar, and ends when its instance does.
    fn served(self: &Arc<Self>, id: &ProgramId, process: &Arc<WindowProcess>) {
        process.show_menu_bar();
        if let Some(instance) = process.instance() {
            self.end_with_its_instance(id.clone(), Arc::clone(process), instance.clone());
        }
    }

    /// When `instance` ends, its window process `process` closes its windows and ends, and `id`
    /// has none until something asks again.
    fn end_with_its_instance(
        self: &Arc<Self>,
        id: ProgramId,
        process: Arc<WindowProcess>,
        instance: AppInstance,
    ) {
        let (table, ended) = (Arc::clone(self), instance.ended());
        self.runtime.spawn(async move {
            ended.await;
            process.end();
            table.forget(&id, &process);
        });
    }

    /// `process`'s window process is gone: every view shown there goes with it, and so does its
    /// instance, which nothing could show any more. WaOS ends a window process only once its
    /// instance has ended, or nothing is shown there, so one gone first was ended by the system or
    /// the user: a crash, or Force Quit.
    pub(super) fn disconnected(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
        tracing::info!(app = %id, "the app's window process is gone: its instance ends with it");
        process.gone();
        self.forget(id, process);
        if let Some(instance) = process.instance() {
            instance.stop();
        }
    }

    /// `process`, `id`'s window process, is gone, or its instance ended: `id` has none until
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

    fn take_launching(&self, id: &ProgramId) -> Option<Launching> {
        let mut apps = self.apps();
        match apps.remove(id)? {
            App::Launching(launching) => Some(launching),
            connected @ App::Connected(_) => {
                apps.insert(id.clone(), connected);
                None
            }
        }
    }

    fn apps(&self) -> MutexGuard<'_, HashMap<ProgramId, App>> {
        self.apps.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The engine's placement on macOS: an app's windows open in its window process, a window that
/// takes the focus bringing it forward as it opens.
impl ViewPlacement for WindowProcesses {
    fn view_requested(&self, program: &AppInstance, request: ViewRequest) -> Result<(), ViewError> {
        if request.config.visible && request.config.activating {
            let_come_forward(program);
        }
        let asking = program.clone();
        self.with_instance(program, move |app| match app {
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
    fn set(&self, program: &AppInstance, _entries: &[MenuEntry]) {
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
        program: &AppInstance,
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
pub(super) fn app_id(program: &AppInstance) -> Result<ProgramId, String> {
    program
        .name()
        .parse()
        .map_err(|_| "a program with no id has no app".to_owned())
}

/// Lets `program`'s window process take WaOS's activation as what the user must see shows there.
pub(super) fn let_come_forward(program: &AppInstance) {
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
