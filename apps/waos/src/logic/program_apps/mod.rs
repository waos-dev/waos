//! Each running program's app of its own, on macOS, as Chrome's app shims are: the program's
//! windows open in a window process of its own, launched from the program's bundle (`bundles`)
//! through Launch Services, and served by WaOS once it said hello over Mach (`server`,
//! `window_process`); what each view shows stays in WaOS, on a canvas WaOS keeps in a layer context
//! the window shows (`canvases`), a window's new size showing only with the program's picture at
//! it, which the window waits for inside the resize (`canvas_pictures`); and so do WaOS's prompts
//! for the program, which its app shows in dialogs of their own, reporting back what each dialog
//! does. The program's menu bar and its context menus are its app's too, macOS showing the bar of
//! the app in front: the bar is sent whole each time the program sets it while its app runs, and as
//! the app connects; a context menu opens there and is answered once it closes, or with none once
//! the app goes or while it does not run (`awaited_answers`). So do its file dialogs: each opens
//! there, a sheet on the window of the view it names or on its own, and is answered with the paths
//! the user picked once it closes, which the engine mounts, or with why none were (`file_dialogs`).
//! So do its notifications: each is posted, scheduled or removed there, under the program's bundle,
//! a post answered once the system has it, `denied` where macOS refuses the program, macOS's own
//! question for them put there after WaOS's, and a click on one goes from there to the program
//! (`notifications`); whatever the app says macOS said of them is kept for the program's permission
//! and told to WaOS's settings (`notification_permissions`), each app that runs reading it again
//! when WaOS asks, as WaOS comes to the front.
//!
//! Everything that reaches a program's app goes through one getter (`app_reach`): WaOS launches the
//! app the first time something needs it, a window, a file dialog, one of WaOS's prompts or a
//! notification, with no window of its own and not brought forward, and everything that asks while
//! it comes waits for that launch; a window that takes the focus, or a file dialog, brings the app
//! forward as it shows, as a window a user opens does. An app the user opens alone, from the Dock
//! or the Finder, finds WaOS, or starts it, and says hello; WaOS runs its program as the library
//! opens it, and so it does for an app macOS launched for a click on one of its notifications,
//! which the hello carries and the program then hears. Every hello is checked: the process that
//! sent it must be the program's bundle as WaOS signed it, by the code hash WaOS kept, which the
//! record's HMAC must vouch for first. A launch that fails, or an app that ends before its hello,
//! has its bundle rebuilt and is launched once more, as Chrome does; a second failure fails
//! everything that waited. An app that ends after its hello, while its program runs, was crashed or
//! force quit: the program ends with it, as nothing could show it any more.
//!
//! Where there can be no apps, as when WaOS finds no window library or loader, or the system lacks
//! the layer contexts a window shows its view's layers through, every window a program asks for
//! fails with why ([`NoProgramApps`]): macOS has no second route, and WaOS shows no program's
//! windows itself.
//!
//! The invariant a later editor will break: a program has one app at a time, which WaOS launches
//! only through the getter, and everything that asks for it is answered exactly once: with the
//! app, which answers it, or with why the app never came.

mod app_reach;
mod awaited_answers;
mod bundles;
mod canvas_pictures;
mod canvases;
mod file_dialogs;
mod notifications;
mod server;
mod window_process;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use anyhow::{Context, Result};
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{RunningProgram, ViewPlacement, ViewRequest};
use waos_package::ProgramId;
use waos_services::packages::PackageManager;
use waos_window_messages::bundle::LAUNCHED_BY_WAOS;
use waos_window_messages::os::macos::AuditToken;
use waos_window_messages::{Hello, LaunchReason, bootstrap_name};

use crate::os::{
    WaosFiles, check_layer_contexts, code_hash_of_process, open_app, safe_storage_key,
    system_version, wait_for_exit,
};

use super::app_starter::{AppStarter, Opening, StartPlace, StartRequest};
use super::program_places::ProgramPlace;
pub(crate) use crate::logic::notification_permissions::{
    NotificationPermissionReading, NotificationPermissions,
};
pub(crate) use app_reach::LaunchAttempt;
pub(crate) use app_reach::{AppReach, ReachedApp};
use bundles::{BundleFolders, Bundles, KeptCodeHash, ProgramDescription, made_from};
pub(crate) use canvas_pictures::{CanvasPictures, ToShow};
pub(crate) use canvases::{CanvasCall, CanvasKey, CanvasPlace, CanvasesOnRequest};
pub(crate) use notifications::AppNotifications;
use window_process::SendToWindow;
pub(crate) use window_process::{
    DialogReport, DialogReports, WindowPresence, WindowPresences, WindowProcess,
};
#[cfg(test)]
pub(crate) use {
    app_reach::tests::{Launches, launch_failed, said_hello},
    window_process::tests::{Compositing, Served, window_process},
};

/// How many threads launch the programs' apps and wait for the programs' answers.
const THREADS: usize = 2;

/// Where WaOS keeps what the programs' apps need.
pub(crate) struct AppsFolders {
    /// WaOS's data folder, which names the port its programs' apps find it by.
    pub(crate) data: PathBuf,
    /// Where the installed programs' apps go.
    pub(crate) installed: PathBuf,
}

pub(crate) struct ProgramApps {
    me: Weak<ProgramApps>,
    bundles: Bundles,
    packages: Arc<PackageManager>,
    /// Where the canvases of the programs' views are kept, on the main thread.
    canvases: Arc<CanvasesOnRequest>,
    /// Where what the dialogs of WaOS's prompts report goes, on the main thread.
    dialog_reports: DialogReports,
    /// Where what the apps say macOS said of the programs' notifications is kept.
    notification_permissions: NotificationPermissions,
    /// Where whether each of the programs' windows is in sight goes, on the main thread.
    window_presences: WindowPresences,
    /// What opens an installed program the user opened the app of, once the engine is made.
    starter: OnceLock<Arc<AppStarter>>,
    runtime: tokio::runtime::Runtime,
    /// Each program's app as what WaOS shows for it reaches it, launched here.
    reach: Arc<AppReach>,
}

impl ProgramApps {
    /// The programs' apps, their bundles in `folders`, the installed ones' read from `store`, the
    /// canvases of their views kept by `canvases`, what their prompts' dialogs report sent to
    /// `dialog_reports`, what they say macOS said of the programs' notifications kept in
    /// `notification_permissions`, whether each of their windows is in sight to `window_presences`, and
    /// listening for their hellos from now on; every bundle is rebuilt if the system changed
    /// since it was signed.
    pub(crate) fn start(
        folders: AppsFolders,
        packages: Arc<PackageManager>,
        canvases: CanvasesOnRequest,
        dialog_reports: DialogReports,
        notification_permissions: NotificationPermissions,
        window_presences: WindowPresences,
    ) -> Result<Arc<ProgramApps>> {
        let waos = waos_files(&folders.data)?;
        check_layer_contexts().map_err(|lacking| {
            anyhow::anyhow!("a program's window cannot show what it draws: {lacking}")
        })?;
        let bundle_folders = BundleFolders {
            installed: folders.installed,
        };
        let key = safe_storage_key().context("the key the programs' apps are vouched for with")?;
        let made_from = made_from(&waos).context("what the programs' apps are made from")?;
        let bundles = Bundles::open(
            bundle_folders,
            (waos, made_from),
            &folders.data,
            system_version(),
            key,
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(THREADS)
            .thread_name("waos-program-apps")
            .enable_all()
            .build()
            .context("the programs' apps' runtime")?;
        let apps = Arc::new_cyclic(|me: &Weak<ProgramApps>| {
            let launching = me.clone();
            let reach = AppReach::new(move |id, attempt| {
                if let Some(apps) = launching.upgrade() {
                    apps.launch(id.clone(), attempt);
                }
            });
            ProgramApps {
                me: me.clone(),
                bundles,
                packages,
                canvases: Arc::new(canvases),
                dialog_reports,
                notification_permissions,
                window_presences,
                starter: OnceLock::new(),
                runtime,
                reach: Arc::new(reach),
            }
        });
        server::listen(&apps, &bootstrap_name(&folders.data))
            .context("listening for the programs' apps")?;
        apps.rebuild_stale();
        apps.follow_installed();
        Ok(apps)
    }

    /// Opens with `starter` the installed programs whose app the user opens alone.
    pub(crate) fn open_with(&self, starter: Arc<AppStarter>) {
        let _ = self.starter.set(starter);
    }

    /// From now on, each program the package manager has installed has its app, at its version,
    /// and a program removed has none: now, and again at each change it announces.
    fn follow_installed(&self) {
        let (me, mut changes) = (self.me.clone(), self.packages.installed_changes());
        self.runtime.spawn(async move {
            loop {
                changes.borrow_and_update();
                let Some(apps) = me.upgrade() else {
                    return;
                };
                // The bundles are files, written off the runtime's workers.
                let matched = tokio::task::spawn_blocking(move || apps.match_installed());
                if matched.await.is_err() || changes.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    /// Makes the bundles match what is installed now.
    fn match_installed(&self) {
        let installed: Vec<ProgramId> = self
            .packages
            .installed()
            .iter()
            .map(|program| program.manifest.id().clone())
            .collect();
        for removed in self.bundles.programs() {
            if !installed.contains(&removed) {
                log_failure(&removed, self.bundles.delete(&removed));
            }
        }
        for id in installed {
            if let Some(description) = self.describe(&id) {
                log_failure(&id, self.bundles.ensure(&description).map(drop));
            }
        }
    }

    /// How what WaOS shows for its programs reaches their apps: the engine's placement, menu bar,
    /// context menus and file dialogs on macOS, its notifications' route, and where WaOS's
    /// prompts are shown.
    pub(crate) fn reach(&self) -> Arc<AppReach> {
        Arc::clone(&self.reach)
    }

    /// What the apps said macOS said of the programs' notifications.
    pub(crate) fn notification_permissions(&self) -> NotificationPermissions {
        self.notification_permissions.clone()
    }

    /// Launches `id`'s app through Launch Services, not brought forward, its bundle written first
    /// if it has none, or written again for a launch after one that failed.
    ///
    /// TODO(step 5): a window, a file dialog, a prompt and a notification are the only reasons yet
    /// to launch a program's app. A program with no window still needs its app when it shows a
    /// tray icon, which its app holds; it then asks the getter too, as a notification does, and as
    /// Chrome's `AppShimManager::LaunchNotificationProvider` launches a shim for its
    /// notifications.
    fn launch(&self, id: ProgramId, attempt: LaunchAttempt) {
        let Some(apps) = self.me.upgrade() else {
            return;
        };
        self.runtime.spawn(async move {
            let Some(description) = apps.describe(&id) else {
                return apps
                    .reach
                    .give_up(&id, "it is not installed, so it has no app");
            };
            let bundle = match attempt {
                LaunchAttempt::First => apps.bundles.ensure(&description),
                LaunchAttempt::Again => apps.bundles.write(&description),
            };
            let bundle = match bundle {
                Ok(bundle) => bundle,
                Err(error) => {
                    let reason = format!("its app could not be written: {error}");
                    return apps.reach.give_up(&id, &reason);
                }
            };
            tracing::info!(program = %id, ?attempt, "launching the program's app");
            match open_app(&bundle.path, &[LAUNCHED_BY_WAOS]).await {
                Ok(opened) => apps.launched(&id, opened.pid),
                Err(error) => apps
                    .reach
                    .launch_failed(&id, &format!("Launch Services: {error}")),
            }
        });
    }

    /// The app of `id` started as the process `pid`: an end before its hello is a failed launch.
    fn launched(&self, id: &ProgramId, pid: i32) {
        self.reach.started(id, pid);
        let (reach, id) = (Arc::downgrade(&self.reach), id.clone());
        let watching = std::thread::Builder::new()
            .name(format!("waos-app {pid}"))
            .spawn(move || {
                let _ = wait_for_exit(pid);
                if let Some(reach) = reach.upgrade() {
                    reach.ended(&id, pid);
                }
            });
        if let Err(error) = watching {
            tracing::warn!(%error, "the program's app's end is not watched");
        }
    }

    /// The program a hello says it is the app of, once its code is checked: the one WaOS launched
    /// it for, or, for an app the user opened or a click on one of the program's notifications
    /// launched, the one running, or the installed one, opened now.
    async fn program_for(
        &self,
        hello: &Hello,
        sender: &AuditToken,
    ) -> Result<RunningProgram, String> {
        self.check_code(&hello.program, sender)?;
        match &hello.launched_for {
            LaunchReason::ProgramRunning => self
                .reach
                .launching(&hello.program)
                .ok_or_else(|| "WaOS launched no app for it".to_owned()),
            LaunchReason::UserOpened | LaunchReason::NotificationActivated(_) => {
                self.running_or_opened(&hello.program).await
            }
        }
    }

    /// Whether the process that sent `sender`'s message is `id`'s bundle as WaOS signed it, by the
    /// code hash the record keeps, once its HMAC vouches for it. One that is not, or whose record
    /// its HMAC does not vouch for, has its bundle rebuilt, as Chrome rebuilds a shim it refused.
    fn check_code(&self, id: &ProgramId, sender: &AuditToken) -> Result<(), String> {
        let kept = match self.bundles.code_hash(id) {
            KeptCodeHash::None => return Err("WaOS wrote no app for it".to_owned()),
            KeptCodeHash::Signed(kept) => Some(kept),
            KeptCodeHash::Unvouched => None,
        };
        if let Some(kept) = kept {
            let running = code_hash_of_process(sender)
                .map_err(|error| format!("its code could not be checked: {error}"))?;
            if running == kept {
                return Ok(());
            }
        }
        self.rebuild(id);
        Err("its code is not the app WaOS signed".to_owned())
    }

    /// Writes `id`'s bundle again, in the background.
    fn rebuild(&self, id: &ProgramId) {
        let Some(apps) = self.me.upgrade() else {
            return;
        };
        let id = id.clone();
        self.runtime.spawn_blocking(move || {
            if let Some(description) = apps.describe(&id) {
                log_failure(&id, apps.bundles.write(&description).map(drop));
            }
        });
    }

    async fn running_or_opened(&self, id: &ProgramId) -> Result<RunningProgram, String> {
        let starter = self
            .starter
            .get()
            .ok_or_else(|| "WaOS is still starting".to_owned())?;
        let request = StartRequest {
            place: StartPlace::Named(ProgramPlace::Windows),
            ..StartRequest::open(id.clone(), Opening::ByTheUser)
        };
        let opened = starter
            .start(request)
            .await
            .map_err(|error| format!("{error:#}"))?;
        Ok(opened.program().clone())
    }

    /// `program`'s app is connected: its window process serves the program, and is handed to what
    /// waited for it, and told when the program ends.
    fn connected(
        &self,
        id: &ProgramId,
        program: RunningProgram,
        send: SendToWindow,
    ) -> Arc<WindowProcess> {
        let process = WindowProcess::new(
            program.clone(),
            send,
            Arc::clone(&self.canvases),
            self.dialog_reports.clone(),
            self.notification_permissions.clone(),
            self.window_presences.clone(),
            self.runtime.handle().clone(),
        );
        self.reach.connected(id, &process);
        self.tell_when_ended(id.clone(), Arc::clone(&process));
        process
    }

    fn tell_when_ended(&self, id: ProgramId, process: Arc<WindowProcess>) {
        let reach = Arc::downgrade(&self.reach);
        let ended = process.program().ended();
        self.runtime.spawn(async move {
            ended.await;
            process.program_ended();
            if let Some(reach) = reach.upgrade() {
                reach.forget(&id, &process);
            }
        });
    }

    /// `process`'s app is gone: every view shown there goes with it, and so does the program,
    /// which nothing could show any more. WaOS ends an app only once its program has ended, so an
    /// app gone first was ended by the system or the user: a crash, or Force Quit.
    fn disconnected(&self, id: &ProgramId, process: &Arc<WindowProcess>) {
        tracing::info!(program = %id, "the program's app is gone: the program ends with it");
        process.gone();
        self.reach.forget(id, process);
        process.program().stop();
    }

    /// What `id`'s bundle is written from: its install, a development install's as any other's;
    /// none for a program not installed, which has no app, since every program that runs is
    /// installed.
    fn describe(&self, id: &ProgramId) -> Option<ProgramDescription> {
        let program = self.packages.installed_program(id)?;
        Some(ProgramDescription {
            id: id.clone(),
            name: program.name(),
            version: program.manifest.version().to_string(),
            icon: self.packages.icon(id),
            category: program.manifest.category(),
            file_associations: program.manifest.file_associations().to_vec(),
        })
    }

    /// Rebuilds, in the background, every bundle signed on another version of the system, and each
    /// made from other files of WaOS's than these, so an app the user opens from the Dock loads
    /// this WaOS's window code.
    fn rebuild_stale(&self) {
        let stale = self.bundles.stale();
        if stale.is_empty() {
            return;
        }
        let Some(apps) = self.me.upgrade() else {
            return;
        };
        tracing::info!(
            count = stale.len(),
            "the programs' apps were signed on another system or made from another WaOS: rebuilding them"
        );
        self.runtime.spawn_blocking(move || {
            for id in stale {
                match apps.describe(&id) {
                    Some(description) => {
                        log_failure(&id, apps.bundles.write(&description).map(drop));
                    }
                    // A bundle of a program no longer installed, as one run in development
                    // under its id before development installs: it goes.
                    None => log_failure(&id, apps.bundles.delete(&id)),
                }
            }
        });
    }
}

/// The engine's placement on macOS where there can be no apps: every window a program asks for
/// fails at once, with why.
pub(crate) struct NoProgramApps {
    why: String,
}

impl NoProgramApps {
    pub(crate) fn new(why: String) -> NoProgramApps {
        NoProgramApps { why }
    }
}

impl ViewPlacement for NoProgramApps {
    fn view_requested(
        &self,
        _program: &RunningProgram,
        _request: ViewRequest,
    ) -> Result<(), ViewError> {
        Err(ViewError::Failed(format!(
            "no program's windows can be shown: {}",
            self.why
        )))
    }
}

/// WaOS's own files the programs' bundles refer to: its window library and the loader, in its app
/// bundle (`Contents/Frameworks`, `Contents/Helpers`) or, run outside one, beside its executable,
/// as a build leaves them; and WaOS to start, the bundle or the executable.
fn waos_files(data_folder: &Path) -> Result<WaosFiles> {
    // Through a link (`~/.local/bin/waos`, which install.sh makes), `current_exe` is the link's
    // path on macOS: its target is where the bundle is.
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .context("WaOS's own executable")?;
    let app = executable
        .ancestors()
        .nth(3)
        .filter(|app| app.extension().is_some_and(|extension| extension == "app"));
    let files = match app {
        Some(app) => WaosFiles {
            loader: app.join("Contents/Helpers/waos-program-loader"),
            window_library: app.join("Contents/Frameworks/libwaos_window.dylib"),
            waos: app.to_path_buf(),
            data_folder: data_folder.to_path_buf(),
        },
        None => {
            let beside = executable.with_file_name("");
            WaosFiles {
                loader: beside.join("waos-program-loader"),
                window_library: beside.join("libwaos_window.dylib"),
                waos: executable.clone(),
                data_folder: data_folder.to_path_buf(),
            }
        }
    };
    for file in [&files.loader, &files.window_library] {
        anyhow::ensure!(
            file.is_file(),
            "{} is not there: build the programs' window process with `cargo build -p waos-window`, which leaves the window library and the loader beside WaOS's executable",
            file.display()
        );
    }
    Ok(files)
}

fn log_failure(program: &ProgramId, result: io::Result<()>) {
    if let Err(error) = result {
        tracing::warn!(%program, %error, "the program's app could not be kept");
    }
}

#[cfg(test)]
mod tests;
