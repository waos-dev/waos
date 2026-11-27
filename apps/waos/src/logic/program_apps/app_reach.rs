//! How what WaOS shows for a program reaches the program's app: through one getter, which launches
//! the app the first time something needs it, with no window and not brought forward, shares that
//! launch with everything that asks while it comes, and answers why if the app never comes; and a
//! second, which never launches, answering the app only while it runs. Each route is a line on top
//! of them: the program's windows, its file dialogs (`file_dialogs`) and WaOS's prompts for it
//! (`state::prompt_dialogs`) take the first; its context menus, which open at a view the app
//! already shows, and its menu bar, which is sent as the app connects, take the second. What the
//! user must see brings the app forward as it shows, by macOS's cooperative activation: WaOS yields
//! its activation to the app as it asks for a window that takes the focus, or a file dialog, and
//! the app takes it as the window or the dialog opens, so a launch for anything else stays in the
//! background; a prompt's dialog in an app in the background bounces its Dock icon instead.
//!
//! The launches are `ProgramApps`'s, which tells this whether an app started, failed or said
//! hello: a launch that fails, or an app that ends before its hello, is launched once more, its
//! bundle written again, as Chrome does; a second failure fails everything that waited.
//!
//! The invariant a later editor will break: a program has one app at a time, launched once
//! however many ask while it comes, and everything that asks is answered exactly once, in the
//! order it asked: with the app, or with why it never came.

use std::collections::HashMap;
use std::future::ready;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::FutureExt;
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::views::{ViewError, ViewId};
use waos::{
    MenuChosenFuture, RunningProgram, ShellContextMenus, ShellMenuBar, ViewPlacement, ViewRequest,
};
use waos_package::ProgramId;

use super::window_process::WindowProcess;
use crate::os::yield_activation_to;

/// What reaching a program's app answers: the window process its app is, or why it never came.
pub(crate) type ReachedApp = Result<Arc<WindowProcess>, String>;

/// A program's app, once it is there, for a route that awaits it.
pub(crate) type AppFuture = Pin<Box<dyn Future<Output = ReachedApp> + Send>>;

/// Which launch of a program's app this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchAttempt {
    First,
    /// The one after a launch that failed, whose bundle is written again.
    Again,
}

/// Why something that waited for a program's app hears it went to another run of the program.
const ANOTHER_RUN: &str = "another run of the program took its app";

/// Something waiting for a program's app.
type Waiting = Box<dyn FnOnce(ReachedApp) + Send>;

/// What launches a program's app, once or again.
type Launch = Box<dyn Fn(&ProgramId, LaunchAttempt) + Send + Sync>;

/// Each program's app as what WaOS shows for it reaches it.
pub(crate) struct AppReach {
    launch: Launch,
    apps: Mutex<HashMap<ProgramId, App>>,
}

/// A program's app, as WaOS has it.
enum App {
    /// WaOS launched it for `program`; its hello has not come. What asked for it meanwhile waits,
    /// in the order it asked.
    Launching {
        program: RunningProgram,
        waiting: Vec<Waiting>,
        /// The app's process, once Launch Services started it.
        pid: Option<i32>,
        attempt: LaunchAttempt,
    },
    /// It said hello: its window process serves the program.
    Connected(Arc<WindowProcess>),
}

impl AppReach {
    /// The programs' apps, each launched by `launch` when something first needs it.
    pub(crate) fn new(launch: impl Fn(&ProgramId, LaunchAttempt) + Send + Sync + 'static) -> Self {
        AppReach {
            launch: Box::new(launch),
            apps: Mutex::default(),
        }
    }

    /// Hands `program`'s app to `then`: at once while it runs; otherwise once it said hello,
    /// launched with no window and not brought forward the first time something asks, everything
    /// that asks while it comes handed it in the order it asked; or with why it never came.
    pub(crate) fn with_app(
        &self,
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
        (self.launch)(&id, LaunchAttempt::First);
    }

    /// `program`'s app, as [`with_app`](Self::with_app) reaches it, for a route that awaits it.
    pub(crate) fn app(&self, program: &RunningProgram) -> AppFuture {
        let (answer, answered) = oneshot::channel();
        self.with_app(program, move |app| {
            let _ = answer.send(app);
        });
        Box::pin(
            answered.map(|app| {
                app.unwrap_or_else(|_| Err("WaOS let go of the program's app".to_owned()))
            }),
        )
    }

    /// `program`'s app while it runs, once it said hello; none before, or after it went. Never
    /// launches it.
    pub(crate) fn running_app(&self, program: &RunningProgram) -> Option<Arc<WindowProcess>> {
        let id = app_id(program).ok()?;
        match self.apps().get(&id)? {
            App::Connected(process) if process.program() == program => Some(Arc::clone(process)),
            _ => None,
        }
    }

    /// The program `id`'s app is launched for, while it launches.
    pub(super) fn launching(&self, id: &ProgramId) -> Option<RunningProgram> {
        match self.apps().get(id)? {
            App::Launching { program, .. } => Some(program.clone()),
            App::Connected(_) => None,
        }
    }

    /// The app launched for `id` started as the process `pid`.
    pub(super) fn started(&self, id: &ProgramId, pid: i32) {
        if let Some(App::Launching { pid: started, .. }) = self.apps().get_mut(id) {
            *started = Some(pid);
        }
    }

    /// The process `pid` ended: if it was `id`'s app, launched and not yet connected, its launch
    /// failed.
    pub(super) fn ended(&self, id: &ProgramId, pid: i32) {
        let launching = matches!(
            self.apps().get(id),
            Some(App::Launching { pid: Some(started), .. }) if *started == pid
        );
        if launching {
            self.launch_failed(id, "its app ended before it said hello");
        }
    }

    /// The launch of `id`'s app failed, for `reason`: after a first failure it is launched once
    /// more, its bundle written again; after a second, everything that waited fails.
    pub(super) fn launch_failed(&self, id: &ProgramId, reason: &str) {
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
            (self.launch)(id, LaunchAttempt::Again);
        } else {
            self.give_up(id, reason);
        }
    }

    /// `id`'s app will not come, for `reason`: everything that waited for it hears why.
    pub(super) fn give_up(&self, id: &ProgramId, reason: &str) {
        let Some(App::Launching { waiting, .. }) = self.take_launching(id) else {
            return;
        };
        answer(
            waiting,
            &Err(format!("the program's app did not start: {reason}")),
        );
    }

    /// `process`, `id`'s app, said hello: it shows the program's menu bar, and everything that
    /// waited for it gets it, in the order it asked.
    pub(super) fn connected(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
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

    /// `process`, `id`'s app, is gone, or its program ended: `id` has no app until something asks
    /// again.
    pub(super) fn forget(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
        let mut apps = self.apps();
        if matches!(apps.get(id), Some(App::Connected(connected)) if Arc::ptr_eq(connected, process))
        {
            apps.remove(id);
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

/// The engine's placement on macOS: a program's windows open in its app, a window that takes the
/// focus bringing the app forward as it opens.
impl ViewPlacement for AppReach {
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

/// The engine's menu bar on macOS: a program's bar is its app's, shown there as the engine keeps it,
/// the `entries` the program just set, while the app runs; it is shown the bar as it connects.
impl ShellMenuBar for AppReach {
    fn set(&self, program: &RunningProgram, _entries: &[MenuEntry]) {
        if let Some(app) = self.running_app(program) {
            app.show_menu_bar();
        }
    }
}

/// The engine's context menus on macOS: a program's open in its app, at a point of a view shown
/// there; one asked for while its app does not run is answered none.
impl ShellContextMenus for AppReach {
    fn show(
        &self,
        program: &RunningProgram,
        view: ViewId,
        x: f64,
        y: f64,
        entries: &[MenuEntry],
    ) -> MenuChosenFuture {
        match self.running_app(program) {
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

/// Lets `program`'s app take WaOS's activation as what the user must see shows there.
pub(super) fn let_come_forward(program: &RunningProgram) {
    if let Ok(id) = app_id(program) {
        yield_activation_to(&id);
    }
}

/// Hands every one `waiting` what reaching the app answered, in the order it asked.
fn answer(waiting: Vec<Waiting>, reached: &ReachedApp) {
    for then in waiting {
        then(reached.clone());
    }
}

#[cfg(test)]
pub(crate) mod tests;
