//! What an installed app is: its manifest and where it came from, and the folder of its version,
//! whose files never change once it is installed.

use std::path::PathBuf;

use waos_package::{COMPONENT, Manifest, PublisherKey};

use crate::packages::programs;

/// What the name of a development install is shown with, after the app's own.
const DEVELOPMENT_LABEL: &str = " (Development)";

/// An app as installed: its manifest, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledProgram {
    pub manifest: Manifest,
    pub origin: Origin,
}

/// Where an installed app came from: a package, whose publisher's key every update of it must be
/// signed with, or its source, built and installed under its development id by `waos install`,
/// which no package updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Package { publisher: PublisherKey },
    Development { component_sha256: [u8; 32] },
}

impl InstalledProgram {
    /// The key every update of it must be signed with; none for a development install.
    pub fn publisher(&self) -> Option<PublisherKey> {
        match self.origin {
            Origin::Package { publisher } => Some(publisher),
            Origin::Development { .. } => None,
        }
    }

    /// Whether it is a development install, made from its source by `waos install`.
    pub fn is_development(&self) -> bool {
        matches!(self.origin, Origin::Development { .. })
    }

    /// The name its user sees: its manifest's, and a development install's followed by
    /// " (Development)", so it reads apart from the app published under the plain id.
    pub fn name(&self) -> String {
        let name = self.manifest.name();
        match self.origin {
            Origin::Package { .. } => name.to_owned(),
            Origin::Development { .. } => format!("{name}{DEVELOPMENT_LABEL}"),
        }
    }
}

/// An app built from its source, as `waos install` sends it, to install as its development
/// install: its manifest, under the app's own id, its component and its icon, where it has one.
#[derive(Clone, Debug)]
pub struct DevelopmentBuild {
    pub manifest: Manifest,
    pub component: Vec<u8>,
    pub icon: Option<Vec<u8>>,
}

impl DevelopmentBuild {
    /// The manifest it is installed with: its own, under the app's development id.
    pub(crate) fn installed_manifest(manifest: &Manifest) -> Manifest {
        Manifest::new(
            manifest.id().development(),
            manifest.name(),
            manifest.version().clone(),
        )
        .expect("a manifest's name is one already")
        .with_kind(manifest.kind())
        .with_category(manifest.category())
        .with_file_associations(manifest.file_associations().to_vec())
        .expect("a manifest's file associations are checked already")
    }
}

/// One installed version of an app: the folder its package, or its development build, unpacked
/// into, whose files never change once it is installed, and its code folder inside, which the
/// compiler writes its component's code into and which goes with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledVersion {
    pub(crate) folder: PathBuf,
}

impl InstalledVersion {
    /// Its component, which the app runs.
    pub fn component(&self) -> PathBuf {
        self.folder.join(COMPONENT)
    }

    /// Its code folder, for the code compiled from its component.
    pub fn code(&self) -> PathBuf {
        self.folder.join(programs::CODE)
    }
}
