//! The package manager: the apps installed on a WaOS device, and every change to them, as one unit
//! (Android's `PackageManagerService`). It is handed what it needs when it is made: where the apps
//! are kept ([`InstalledAppsStore`]: [`InstalledAppsFiles`] on disk, [`InstalledAppsInMemory`] for
//! tests), a compiler ([`CodeCompiler`], the engine's) and a way to stop an app's instance
//! ([`StopApp`], the WaOS app's mailbox to its app instances); it knows nothing of sources, UI or the engine. Installs in flight are sessions
//! ([`InstallSession`], Android's `PackageInstaller.Session`), fed a package's bytes from its first
//! byte.
//!
//! What a caller relies on:
//! 1. An app is always one whole version; a crash mid-change leaves the old or the new.
//! 2. What is checked is what is installed: a package's bytes are taken into the store's own
//!    file before anything reads them.
//! 3. An update comes from the same publisher, at a newer version.
//! 4. Installed means ready to open: an install ends once its code is compiled.
//! 5. Compiled code is a cache, compiled again whenever it cannot be used.
//! 6. An app is stopped before it changes, and does not start while it changes ([`AppLock`]).
//! 7. Removing takes everything the store keeps with the app.
//! 8. Every change is announced: [`PackageManager::installed_changes`], the sessions, and each
//!    removal.
//! 9. Each session records who asked for it ([`Caller`]).
//!
//! Sessions of different callers do not touch each other, and the update check decides at commit;
//! a caller's newer session for an app replaces its own older one; a removal cancels all the app's
//! sessions. A session does not survive a restart: what it left is cleared when the store opens.

mod error;
mod installed;
mod installed_apps;
mod installed_apps_in_memory;
mod manager;
mod programs;
mod sessions;
mod store;

pub use error::{FolderError, InstallError, RemoveError};
pub use installed::{DevelopmentBuild, InstalledProgram, InstalledVersion, Origin};
pub use installed_apps::{InstalledApps, NothingInstalled};
pub use installed_apps_in_memory::{AppInMemory, InstalledAppsInMemory};
pub use manager::{
    AppLock, ByteStream, CodeCompiler, PackageChange, PackageContent, PackageManager, StopApp,
};
pub use programs::InstalledAppsFiles;
pub use sessions::{Caller, InstallProgress, InstallSession, InstallSessionInfo};
pub use store::{InstalledAppsStore, StagedVersion};
