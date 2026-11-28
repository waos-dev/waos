//! One app bundle per installed app, at its version, following the package manager, as Chrome's
//! `ShortcutSubManager` keeps its app shims: written at install, update, or a change of the kinds
//! of file the app opens, deleted at removal (`bundles`), and, as WaOS starts, rebuilt where the
//! system or WaOS's own files changed since it was signed. A window process is launched from its
//! app's bundle through Launch Services, in the background and not brought forward, the bundle
//! written first where it has none, or written again when asked; and a window process's hello is
//! checked against the bundle WaOS signed, by the code hash the record keeps once its HMAC
//! vouches for it, a bundle refused being rebuilt, as Chrome rebuilds a shim it refused.
//!
//! The invariant an editor could break: nothing here waits on what it was asked from. Bundles are
//! written on the runtime it was handed, a launch's writes inside the launch's own future, never on
//! the caller's thread, which may be the engine's or the main one.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use waos_package::ProgramId;
use waos_services::packages::PackageManager;
use waos_window_messages::bundle::LAUNCHED_BY_WAOS;
use waos_window_messages::os::macos::AuditToken;

use super::bundles::{BundleFiles, BundleFolders, KeptCodeHash, ProgramDescription, made_from};
use super::window_processes::{Bundles, LaunchFailure, LaunchedProcess};
use crate::logic::program_name::name_of;
use crate::os::{
    WaosFiles, code_hash_of_process, open_app, safe_storage_key, system_version, wait_for_exit,
};

/// Where WaOS keeps what the apps' bundles need.
pub(crate) struct AppsFolders {
    /// WaOS's data folder, which names the port its window processes find it by.
    pub(crate) data: PathBuf,
    /// Where the installed apps' bundles go.
    pub(crate) installed: PathBuf,
}

/// One app bundle per installed app, at its version, as the package manager says.
pub(crate) struct AppBundles {
    files: BundleFiles,
    packages: Arc<PackageManager>,
    /// Where bundles are written and the package manager is followed.
    runtime: tokio::runtime::Handle,
}

impl AppBundles {
    /// The bundles in `folders` of the apps `packages` installs, written on `runtime`; why there
    /// can be none, as when WaOS's window library or loader is not there or the Keychain keeps no
    /// key to vouch for their code hashes with.
    pub(crate) fn open(
        folders: AppsFolders,
        packages: Arc<PackageManager>,
        runtime: tokio::runtime::Handle,
    ) -> Result<AppBundles> {
        let waos = waos_files(&folders.data)?;
        let key = safe_storage_key().context("the key the apps' bundles are vouched for with")?;
        let made_from = made_from(&waos).context("what the apps' bundles are made from")?;
        let files = BundleFiles::open(
            BundleFolders {
                installed: folders.installed,
            },
            (waos, made_from),
            &folders.data,
            system_version(),
            key,
        );
        Ok(AppBundles {
            files,
            packages,
            runtime,
        })
    }

    /// From now on, each app the package manager has installed has its bundle, at its version,
    /// and an app removed has none: now, and again at each change it announces. Every bundle
    /// signed on another version of the system, or made from other files of WaOS's than these, is
    /// rebuilt first, so an app the user opens from the Dock loads this WaOS's window code.
    pub(crate) fn follow_installed(self: &Arc<Self>) {
        self.rebuild_stale();
        let (bundles, mut changes) = (Arc::clone(self), self.packages.installed_changes());
        self.runtime.spawn(async move {
            loop {
                changes.borrow_and_update();
                let matching = Arc::clone(&bundles);
                // The bundles are files, written off the runtime's workers.
                let matched = tokio::task::spawn_blocking(move || matching.match_installed());
                if matched.await.is_err() || changes.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    /// Makes the bundles match what is installed now.
    fn match_installed(&self) {
        let installed: Vec<ProgramId> = (self.packages.installed().iter())
            .map(|app| app.manifest.id().clone())
            .collect();
        for removed in self.files.programs() {
            if !installed.contains(&removed) {
                log_failure(&removed, self.files.delete(&removed));
            }
        }
        for id in installed {
            if let Some(description) = self.describe(&id) {
                log_failure(&id, self.files.ensure(&description).map(drop));
            }
        }
    }

    /// Rebuilds, in the background, every bundle signed on another version of the system, and
    /// each made from other files of WaOS's than these; deletes one whose app is no longer
    /// installed.
    fn rebuild_stale(self: &Arc<Self>) {
        let stale = self.files.stale();
        if stale.is_empty() {
            return;
        }
        tracing::info!(
            count = stale.len(),
            "the apps' bundles were signed on another system or made from another WaOS: rebuilding them"
        );
        let bundles = Arc::clone(self);
        self.runtime.spawn_blocking(move || {
            for id in stale {
                match bundles.describe(&id) {
                    Some(description) => {
                        log_failure(&id, bundles.files.write(&description).map(drop));
                    }
                    // A bundle of an app no longer installed, as one run in development under its
                    // id before development installs: it goes.
                    None => log_failure(&id, bundles.files.delete(&id)),
                }
            }
        });
    }

    /// Writes `id`'s bundle again, in the background.
    fn rebuild(self: &Arc<Self>, id: &ProgramId) {
        let (bundles, id) = (Arc::clone(self), id.clone());
        self.runtime.spawn_blocking(move || {
            if let Some(description) = bundles.describe(&id) {
                log_failure(&id, bundles.files.write(&description).map(drop));
            }
        });
    }

    /// Starts `id`'s window process from its bundle, written first where `rewrite` asks or it has
    /// none; answers its process once Launch Services started it.
    ///
    /// TODO(step 5): a window, a file dialog, a prompt and a notification are the only reasons yet
    /// to launch an app's window process. An app with no window still needs it when it shows a
    /// tray icon, which its window process holds; it then asks for it too, as a notification
    /// does, and as Chrome's `AppShimManager::LaunchNotificationProvider` launches a shim for its
    /// notifications.
    async fn launch_now(
        &self,
        id: ProgramId,
        rewrite: bool,
    ) -> Result<LaunchedProcess, LaunchFailure> {
        let description = self.describe(&id).ok_or_else(|| {
            LaunchFailure::Bundle("it is not installed, so it has no app".to_owned())
        })?;
        let bundle = if rewrite {
            self.files.write(&description)
        } else {
            self.files.ensure(&description)
        };
        let bundle = bundle.map_err(|error| {
            LaunchFailure::Bundle(format!("its app could not be written: {error}"))
        })?;
        tracing::info!(program = %id, rewrite, "launching the app's window process");
        let opened = open_app(&bundle.path, &[LAUNCHED_BY_WAOS])
            .await
            .map_err(|error| LaunchFailure::LaunchServices(format!("Launch Services: {error}")))?;
        Ok(LaunchedProcess {
            pid: opened.pid,
            exited: exit_of(opened.pid),
        })
    }

    /// What `id`'s bundle is written from: its install, a development install's as any other's;
    /// none for an app not installed, which has no bundle, since every app that runs is installed.
    fn describe(&self, id: &ProgramId) -> Option<ProgramDescription> {
        let installed = self.packages.installed_program(id)?;
        Some(ProgramDescription {
            id: id.clone(),
            name: installed.name(),
            version: installed.manifest.version().to_string(),
            icon: self.packages.icon(id),
            category: installed.manifest.category(),
            file_associations: installed.manifest.file_associations().to_vec(),
        })
    }
}

impl Bundles for AppBundles {
    fn launch(
        self: Arc<Self>,
        app: ProgramId,
        rewrite: bool,
    ) -> BoxFuture<'static, Result<LaunchedProcess, LaunchFailure>> {
        async move { self.launch_now(app, rewrite).await }.boxed()
    }

    /// Whether the process that sent `sender`'s message is `app`'s bundle as WaOS signed it, by
    /// the code hash the record keeps, once its HMAC vouches for it. One that is not, or whose
    /// record its HMAC does not vouch for, has its bundle rebuilt.
    fn check(self: Arc<Self>, app: &ProgramId, sender: &AuditToken) -> Result<(), String> {
        let kept = match self.files.code_hash(app) {
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
        self.rebuild(app);
        Err("its code is not the app WaOS signed".to_owned())
    }

    fn name(&self, app: &ProgramId) -> String {
        name_of(&self.packages, app.as_str())
    }
}

/// When the process `pid` ends, watched on a thread of its own.
fn exit_of(pid: i32) -> oneshot::Receiver<()> {
    let (exited, exit) = oneshot::channel();
    let watching = std::thread::Builder::new()
        .name(format!("waos-app {pid}"))
        .spawn(move || {
            let _ = wait_for_exit(pid);
            let _ = exited.send(());
        });
    if let Err(error) = watching {
        tracing::warn!(%error, "the window process's end is not watched");
    }
    exit
}

/// WaOS's own files the apps' bundles refer to: its window library and the loader, in its app
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
            "{} is not there: build the apps' window process with `cargo build -p waos-window`, which leaves the window library and the loader beside WaOS's executable",
            file.display()
        );
    }
    Ok(files)
}

fn log_failure(app: &ProgramId, result: io::Result<()>) {
    if let Err(error) = result {
        tracing::warn!(program = %app, %error, "the app's bundle could not be kept");
    }
}
