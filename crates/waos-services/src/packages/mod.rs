//! The package manager: the apps installed on a WaOS device, and every change to them, as one unit
//! (Android's `PackageManagerService`). It is handed what it needs when it is opened: a compiler
//! ([`CodeCompiler`], the engine's) and a way to stop an app ([`StopApp`], the running apps'); it
//! knows nothing of sources, UI or the engine. Installs in flight are sessions ([`InstallSession`],
//! Android's `PackageInstaller.Session`), fed a package's bytes from its first byte.
//!
//! What a caller relies on:
//! 1. An app is always one whole version; a crash mid-change leaves the old or the new.
//! 2. What is checked is what is installed: a package's bytes are taken into the package
//!    manager's own folder before anything reads them.
//! 3. An update comes from the same publisher, at a newer version.
//! 4. Installed means ready to open: an install ends once its code is compiled.
//! 5. Compiled code is a cache, compiled again whenever it cannot be used.
//! 6. An app is stopped before it changes, and does not start while it changes ([`AppLock`]).
//! 7. Removing takes everything kept in the app's folder.
//! 8. Every change is announced: [`PackageManager::installed_changes`], the sessions, and each
//!    removal.
//! 9. Each session records who asked for it ([`Caller`]).
//!
//! Sessions of different callers do not touch each other, and the update check decides at commit;
//! a caller's newer session for an app replaces its own older one; a removal cancels all the app's
//! sessions. A session does not survive a restart: what it left is cleared when the package
//! manager opens.
//!
//! ```text
//! <folder>/
//!   programs/
//!     dev.example.triangle/
//!       current           the name of the folder below that is installed
//!       1.2.0-Xk3q9a/     what the package unpacked to: manifest.toml, program.wasm, icon.png, publisher.key
//!         code/           the code compiled from program.wasm, a file per compiler
//!       settings.toml     Permissions', kept beside the versions and removed with the folder
//!       data/  cache/     the app's own folders, app storage's, the same
//!     dev.example.triangle.development/   its development install, an app of its own
//!   removing/             an app being removed, moved here whole first
//! ```

mod error;
mod files;
mod installed;
mod installed_apps;
mod manager;
mod programs;
mod sessions;

pub use error::{FolderError, InstallError, RemoveError};
pub use installed::{DevelopmentBuild, InstalledProgram, InstalledVersion, Origin};
pub use installed_apps::{InstalledApps, NothingInstalled};
pub use manager::{
    AppLock, ByteStream, CodeCompiler, PackageChange, PackageContent, PackageManager, StopApp,
};
pub use sessions::{Caller, InstallProgress, InstallSession, InstallSessionInfo};
