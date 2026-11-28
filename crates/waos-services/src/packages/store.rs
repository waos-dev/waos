//! Where the package manager keeps the installed apps ([`InstalledAppsStore`]): on disk
//! ([`InstalledAppsFiles`](super::InstalledAppsFiles)), or in memory for tests
//! ([`InstalledAppsInMemory`](super::InstalledAppsInMemory)).
//!
//! The invariant a later editor will break: a store changes an app only whole. Staging changes
//! nothing installed; [`InstalledAppsStore::commit`] is the one step that puts a version in place,
//! and [`InstalledAppsStore::remove`] the one step that takes an app. What may be installed (its
//! app, its publisher, a newer version), and when, is the package manager's to decide, never a
//! store's.

use std::sync::Arc;

use tempfile::{NamedTempFile, TempDir};
use waos_package::{Package, ProgramId};

use crate::packages::error::{FolderError, InstallError, RemoveError};
use crate::packages::installed::{DevelopmentBuild, InstalledProgram, InstalledVersion};

/// Where the package manager keeps the installed apps: each app's installed version, replaced or
/// removed whole. Its calls may block on files, so the package manager makes them off its async
/// tasks wherever they change something.
pub trait InstalledAppsStore: Send + Sync {
    /// Every installed app. One that cannot be read is left out, and said so to whoever
    /// subscribes to `tracing`'s events.
    fn apps(&self) -> Vec<InstalledProgram>;

    /// The installed app `id`; none when it is not installed.
    fn app(&self, id: &ProgramId) -> Result<Option<InstalledProgram>, FolderError>;

    /// The icon of the installed app `id`, a square PNG's bytes; none when it is not installed,
    /// it has none, or it cannot be read, which is said to whoever subscribes.
    fn icon(&self, id: &ProgramId) -> Option<Arc<[u8]>>;

    /// The installed version of `id`: where the engine loads it from; none when it is not
    /// installed, it has no version to run, or it cannot be read, which is said to whoever
    /// subscribes.
    fn version(&self, id: &ProgramId) -> Option<InstalledVersion>;

    /// A new file to receive a package of `app` into before it is opened, which goes when it is
    /// dropped; what a crash left of one is cleared when the store opens.
    fn receiving_file(&self, app: &ProgramId) -> Result<NamedTempFile, FolderError>;

    /// Keeps `package` as a new version of its app, beside the installed one, which does not
    /// change.
    fn stage(&self, package: &mut Package) -> Result<StagedVersion, InstallError>;

    /// Keeps `build` as a new version of its app's development install, under its development id,
    /// beside the installed one, which does not change.
    fn stage_development(&self, build: &DevelopmentBuild) -> Result<StagedVersion, InstallError>;

    /// Puts `staged`, a version this store staged, in the installed one's place, whole.
    fn commit(&self, staged: StagedVersion) -> Result<InstalledProgram, InstallError>;

    /// Removes the app `id` whole, with everything kept with it.
    fn remove(&self, id: &ProgramId) -> Result<(), RemoveError>;
}

/// A version of an app a store keeps beside the installed one, not installed until it is
/// committed; dropped uncommitted, it goes.
pub struct StagedVersion {
    program: InstalledProgram,
    content: StagedContent,
}

/// What a staged version is kept as, by the store that staged it.
pub(crate) enum StagedContent {
    /// Unpacked into a folder of its own, which goes when it is dropped uncommitted.
    Folder(TempDir),
}

impl StagedVersion {
    pub(crate) fn new(program: InstalledProgram, content: StagedContent) -> StagedVersion {
        StagedVersion { program, content }
    }

    /// The app as it is installed once committed.
    pub fn program(&self) -> &InstalledProgram {
        &self.program
    }

    /// The version's files, whose component is compiled before it is committed; none where the
    /// store keeps no files.
    pub fn version(&self) -> Option<InstalledVersion> {
        match &self.content {
            StagedContent::Folder(folder) => Some(InstalledVersion::new(folder.path().to_owned())),
        }
    }

    /// The app and what it is kept as.
    pub(crate) fn into_parts(self) -> (InstalledProgram, StagedContent) {
        (self.program, self.content)
    }
}
