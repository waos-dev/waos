//! macOS's parts of WaOS's shell, as Chrome's app shims are the browser's: each app's windows open
//! in a window process of its own, which belongs to the app, launched from the app's bundle by the
//! app's id (`app_bundles`, `bundles`) through Launch Services, and served by WaOS once it said
//! hello over Mach (`server`, `window_process`), at most one per app, the app's instance attaching
//! to it as it first reaches it (`window_processes`); what each view shows stays in
//! WaOS, on a canvas WaOS keeps in a layer context the window shows (`canvases`), a window's new
//! size showing only with the app's picture at it, which the window waits for inside the resize
//! (`canvas_pictures`); and so do WaOS's prompts for the app, which its window process shows in
//! dialogs of their own, reporting back what each dialog does, whether or not an instance of the
//! app runs. The app's menu bar and its context
//! menus are its window process's too, macOS showing the bar of the app in front: the bar is sent
//! whole each time the app sets it while its window process runs, and as it connects; a context
//! menu opens there and is answered once it closes, or with none once the window process goes or
//! while it does not run (`awaited_answers`). So do its file dialogs: each opens there, a sheet on
//! the window of the view it names or on its own, and is answered with the paths the user picked
//! once it closes, which the engine mounts, or with why none were (`file_dialogs`). So do its
//! notifications: each is posted, scheduled or removed there, under the app's bundle, a post
//! answered once the system has it, `denied` where macOS refuses the app, macOS's own question for
//! them put there after WaOS's, and a click on one goes from there to the app (`notifications`);
//! whatever the window process says macOS said of them is kept for the app's permission and told
//! to WaOS's settings (`notification_permissions`), each window process that runs reading it again
//! when WaOS asks, as WaOS comes to the front.
//!
//! A window process the user opens alone, from the Dock or the Finder, finds WaOS, or starts it,
//! and says hello; WaOS opens its app as the user's open (`system_shell::UserOpen`), and so it
//! does for one macOS launched for a click on one of the app's notifications, which the hello
//! carries and the app's instance then hears. Every hello is checked: the process that sent it must be the
//! app's bundle as WaOS signed it, by the code hash WaOS kept, which the record's HMAC must vouch
//! for first.
//!
//! Where there can be no window processes, as when WaOS finds no window library or loader, or the
//! system lacks the layer contexts a window shows its view's layers through, every window an app
//! asks for fails with why ([`NoWindowProcesses`]): macOS has no second route, and WaOS shows no
//! app's windows itself.
//!
//! The invariant a later editor will break: one chain owns all of it. macOS's parts of the
//! engine's shell hold [`WindowProcesses`], which holds the runtime, the listener, the bundles
//! and each window process; nothing it starts holds it back, so as the engine lets go of its
//! shell, the listener stops and each window process hears its connection end.

mod app_bundles;
mod awaited_answers;
mod bundles;
mod canvas_pictures;
mod canvases;
mod file_dialogs;
mod notifications;
mod server;
mod window_process;
mod window_processes;

use std::sync::Arc;

use anyhow::{Context, Result};
use futures::channel::mpsc::UnboundedSender;
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{AppInstance, ViewPlacement, ViewRequest};
use waos_services::packages::PackageManager;
use waos_window_messages::bootstrap_name;

use crate::os::check_layer_contexts;
use crate::system_shell::UserOpen;

pub(crate) use crate::logic::notification_permissions::{
    NotificationPermissionReading, NotificationPermissions,
};
pub(crate) use app_bundles::{AppBundles, AppsFolders};
pub(crate) use canvas_pictures::{CanvasPictures, ToShow};
pub(crate) use canvases::{CanvasCall, CanvasKey, CanvasPlace, CanvasesOnRequest};
pub(crate) use notifications::AppNotifications;
pub(crate) use window_process::{DialogReport, WindowPresence};
pub(crate) use window_processes::{Routes, WindowProcesses};
#[cfg(test)]
pub(crate) use {
    window_process::tests::{Compositing, Served, window_process},
    window_processes::LaunchAttempt,
    window_processes::tests::{Launches, Listening, said_hello},
};

/// How many threads launch the window processes and wait for the apps' answers.
const THREADS: usize = 2;

/// The apps' window processes, their bundles in `folders`, following what `packages` installs,
/// what each reports going by `routes`, listening for them from now on, the apps the user opens
/// going to `user_opens`; why there can be none, as when WaOS's window library or loader is not
/// there or the system cannot show what a window draws.
pub(crate) fn start(
    folders: AppsFolders,
    packages: Arc<PackageManager>,
    routes: Routes,
    user_opens: UnboundedSender<UserOpen>,
) -> Result<Arc<WindowProcesses>> {
    check_layer_contexts().map_err(|lacking| {
        anyhow::anyhow!("a program's window cannot show what it draws: {lacking}")
    })?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(THREADS)
        .thread_name("waos-window-processes")
        .enable_all()
        .build()
        .context("the window processes' runtime")?;
    let name = bootstrap_name(&folders.data);
    let bundles = Arc::new(AppBundles::open(
        folders,
        packages,
        runtime.handle().clone(),
    )?);
    bundles.follow_installed();
    WindowProcesses::start(runtime, bundles, routes, &name, user_opens)
}

/// The engine's placement on macOS where there can be no window processes: every window an app
/// asks for fails at once, with why.
pub(crate) struct NoWindowProcesses {
    why: String,
}

impl NoWindowProcesses {
    pub(crate) fn new(why: String) -> NoWindowProcesses {
        NoWindowProcesses { why }
    }
}

impl ViewPlacement for NoWindowProcesses {
    fn view_requested(
        &self,
        _program: &AppInstance,
        _request: ViewRequest,
    ) -> Result<(), ViewError> {
        Err(ViewError::Failed(format!(
            "no program's windows can be shown: {}",
            self.why
        )))
    }
}

#[cfg(test)]
mod tests;
