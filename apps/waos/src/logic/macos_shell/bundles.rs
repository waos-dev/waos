//! Each program's app bundle as WaOS keeps it, as Chrome keeps its app shims: an installed app's
//! in a folder of the user's applications, and a tool's in WaOS's data folder, out of the lists of
//! apps the user opens, as Chrome keeps a copy of each shim in its profile; named after the program, with its package's
//! name, version and icon and the kinds of file it opens, a development install's as any other's,
//! since every program that runs is installed. Each is written at install, or at the first launch
//! that finds none, rewritten when the program is updated or the kinds of file it opens change, as
//! a development install again may change them at the same version, and when WaOS's files it is
//! made from are not those it was written from, as Chrome rewrites its shims when Chrome updates:
//! the loader it carries a copy of, by its SHA-256, and the window library and the WaOS it names,
//! by their paths ([`made_from`]); and deleted when it is removed. WaOS keeps, in its data folder,
//! where each bundle is and the code hash it was signed with, which its process is checked against
//! at each connection, and the system's version it was signed on: when the system's version
//! changes, every bundle is rebuilt, since Gatekeeper then treats the ad-hoc signatures made before
//! as unverified (Chrome's commit `91dc901582`).
//!
//! Each code hash is kept with its HMAC-SHA256 under WaOS's key in the Keychain, as Chrome's
//! `AppShimRegistry` keeps `cdhash_hmac` under a key OSCrypt keeps in the Keychain: another program
//! of the user's could write the record, but not vouch for what it wrote. A hash its HMAC does not
//! vouch for is no hash WaOS signed with ([`KeptCodeHash::Unvouched`]).
//!
//! The invariant a later editor will break: the record names only bundles WaOS wrote and signed,
//! each with the hash it was signed with and that hash's HMAC; a bundle is recorded after it is in
//! place, and taken out of the record before it is deleted.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use waos_package::{Category, FileAssociation, ProgramId, ProgramKind};

use crate::os::{BundleContents, WaosFiles, delete_bundle, write_bundle};

/// The record's file in WaOS's data folder.
const RECORD: &str = "program-apps.toml";

/// What a program's bundle is written from.
#[derive(Clone, Debug)]
pub(crate) struct ProgramDescription {
    pub(crate) id: ProgramId,
    /// Its name as its user knows it.
    pub(crate) name: String,
    pub(crate) version: String,
    /// Its icon, a PNG's bytes, as its install keeps it; none for a development build that had
    /// none.
    pub(crate) icon: Option<Arc<[u8]>>,
    /// What it is for, where its author said, which macOS groups its apps by.
    pub(crate) category: Option<Category>,
    /// The kinds of file it opens, which the Finder offers its app for.
    pub(crate) file_associations: Vec<FileAssociation>,
    /// Whether it is an app the user opens, or a tool, run from a terminal.
    pub(crate) kind: ProgramKind,
}

/// Where the bundles go.
#[derive(Clone, Debug)]
pub(crate) struct BundleFolders {
    /// Installed apps': a folder of the user's applications.
    pub(crate) installed: PathBuf,
    /// Installed tools': a folder of WaOS's own, which no list of apps the user opens shows.
    pub(crate) tools: PathBuf,
}

/// What the record says of the code hash a program's bundle was signed with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeptCodeHash {
    /// WaOS wrote no bundle for the program.
    None,
    /// The hash WaOS signed the bundle with, as its HMAC vouches.
    Signed(Vec<u8>),
    /// A hash its HMAC does not vouch for: WaOS did not write it.
    Unvouched,
}

/// The bundles WaOS wrote, and the record of them.
pub(crate) struct BundleFiles {
    folders: BundleFolders,
    waos: WaosFiles,
    /// The key each code hash's HMAC is made under.
    key: Vec<u8>,
    record_file: PathBuf,
    record: Mutex<Record>,
    /// The system's version now.
    system: String,
    /// What WaOS's files a bundle is made from are now ([`made_from`]).
    made_from: String,
}

/// What WaOS keeps of the bundles it wrote.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Record {
    /// The system's version the bundles were signed on.
    system: String,
    /// Each bundle, by its program's id.
    apps: BTreeMap<String, KeptBundle>,
}

/// One bundle WaOS wrote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct KeptBundle {
    pub(crate) path: PathBuf,
    /// Its code hash, in hex.
    code_hash: String,
    /// The HMAC-SHA256 of its code hash under WaOS's key, in hex.
    code_hash_hmac: String,
    /// The program's version it was written for.
    version: String,
    /// The kinds of file it was written to open; none for a bundle written before WaOS kept them,
    /// which opened none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    file_associations: Vec<FileAssociation>,
    /// What WaOS's files it was made from were ([`made_from`]); none for a bundle written before
    /// WaOS kept it, which is made again.
    #[serde(default)]
    made_from: String,
}

impl BundleFiles {
    /// The bundles in `folders`, referring to `waos`'s files, which are `made_from` now, as the
    /// record in `data_folder` says they were written, on the system at `system`, their code
    /// hashes vouched for under `key`.
    pub(crate) fn open(
        folders: BundleFolders,
        (waos, made_from): (WaosFiles, String),
        data_folder: &Path,
        system: String,
        key: Vec<u8>,
    ) -> BundleFiles {
        let record_file = data_folder.join(RECORD);
        let record = read_record(&record_file);
        BundleFiles {
            folders,
            waos,
            key,
            record_file,
            record: Mutex::new(record),
            system,
            made_from,
        }
    }

    /// Whether the bundles were signed on another version of the system than this one.
    pub(crate) fn signed_on_another_system(&self) -> bool {
        let record = self.record();
        !record.apps.is_empty() && record.system != self.system
    }

    /// The programs whose bundle is to be written again: every one where they were signed on
    /// another version of the system, and otherwise each made from other files of WaOS's than
    /// these.
    pub(crate) fn stale(&self) -> Vec<ProgramId> {
        if self.signed_on_another_system() {
            return self.programs();
        }
        self.record()
            .apps
            .iter()
            .filter(|(_, kept)| kept.made_from != self.made_from)
            .filter_map(|(id, _)| id.parse().ok())
            .collect()
    }

    /// The programs that have a bundle.
    pub(crate) fn programs(&self) -> Vec<ProgramId> {
        self.record()
            .apps
            .keys()
            .filter_map(|id| id.parse().ok())
            .collect()
    }

    /// The bundle of `program`, as WaOS wrote it, at the version it is now, opening the kinds of
    /// file it opens now, from WaOS's files as they are now; written now if there is none, or it
    /// is of another version, opens other kinds of file, or is made from other files.
    pub(crate) fn ensure(&self, program: &ProgramDescription) -> io::Result<KeptBundle> {
        match self.kept(&program.id) {
            Some(kept) if self.is_current(&kept, program) => Ok(kept),
            _ => self.write(program),
        }
    }

    /// Whether `kept`, `program`'s bundle, is written as it is now: at its version, opening the
    /// kinds of file it opens, from WaOS's files as they are now, and in place; a tool's not
    /// among the user's applications.
    fn is_current(&self, kept: &KeptBundle, program: &ProgramDescription) -> bool {
        kept.version == program.version
            && kept.file_associations == program.file_associations
            && kept.made_from == self.made_from
            && (program.kind == ProgramKind::App || !self.among_applications(kept))
            && kept.path.exists()
    }

    /// Whether `program`'s bundle, if WaOS wrote one, is among the user's applications, as an
    /// older WaOS wrote a tool's too.
    pub(crate) fn listed(&self, program: &ProgramId) -> bool {
        self.kept(program)
            .is_some_and(|kept| self.among_applications(&kept))
    }

    /// Whether `kept` is in the folder of the user's applications that WaOS writes apps in.
    fn among_applications(&self, kept: &KeptBundle) -> bool {
        kept.path.parent() == Some(self.folders.installed.as_path())
    }

    /// Writes the bundle of `program` again, in place of the one there.
    pub(crate) fn write(&self, program: &ProgramDescription) -> io::Result<KeptBundle> {
        let path = self.place_of(program);
        let contents = BundleContents {
            program: &program.id,
            name: &program.name,
            version: &program.version,
            icon: program.icon.as_deref(),
            category: program.category,
            file_associations: &program.file_associations,
            waos: &self.waos,
        };
        if let Some(before) = self.kept(&program.id).filter(|before| before.path != path) {
            delete_bundle(&before.path, &program.id)?;
        }
        let code_hash = write_bundle(&path, &contents)?;
        let kept = KeptBundle {
            path,
            code_hash_hmac: hex::encode(self.hmac(&code_hash).finalize().into_bytes()),
            code_hash: hex::encode(code_hash),
            version: program.version.clone(),
            file_associations: program.file_associations.clone(),
            made_from: self.made_from.clone(),
        };
        tracing::info!(program = %program.id, path = %kept.path.display(), "the program's app was written");
        self.change_record(|record| {
            record.system.clone_from(&self.system);
            record.apps.insert(program.id.to_string(), kept.clone());
        })?;
        Ok(kept)
    }

    /// What the record says of the code hash `program`'s bundle was signed with, once its HMAC is
    /// checked.
    pub(crate) fn code_hash(&self, program: &ProgramId) -> KeptCodeHash {
        let Some(kept) = self.kept(program) else {
            return KeptCodeHash::None;
        };
        let (Ok(code_hash), Ok(hmac)) = (
            hex::decode(&kept.code_hash),
            hex::decode(&kept.code_hash_hmac),
        ) else {
            return KeptCodeHash::Unvouched;
        };
        match self.hmac(&code_hash).verify_slice(&hmac) {
            Ok(()) => KeptCodeHash::Signed(code_hash),
            Err(_) => KeptCodeHash::Unvouched,
        }
    }

    /// The HMAC-SHA256 of `code_hash` under WaOS's key.
    fn hmac(&self, code_hash: &[u8]) -> Hmac<Sha256> {
        let mut hmac =
            Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC takes a key of any length");
        hmac.update(code_hash);
        hmac
    }

    /// Deletes `program`'s bundle, if WaOS wrote one.
    pub(crate) fn delete(&self, program: &ProgramId) -> io::Result<()> {
        let Some(kept) = self.kept(program) else {
            return Ok(());
        };
        self.change_record(|record| {
            record.apps.remove(program.as_str());
        })?;
        tracing::info!(%program, "the program's app is deleted");
        delete_bundle(&kept.path, program)
    }

    fn kept(&self, program: &ProgramId) -> Option<KeptBundle> {
        self.record().apps.get(program.as_str()).cloned()
    }

    /// Where `program`'s bundle goes: named after it in the folder of its kind, unless another
    /// program's bundle has that name, when its id is added.
    fn place_of(&self, program: &ProgramDescription) -> PathBuf {
        let folder = self.folder_of(program);
        let named = folder.join(format!("{}.app", file_name(&program.name)));
        let taken = self
            .record()
            .apps
            .iter()
            .any(|(id, kept)| kept.path == named && id != program.id.as_str());
        if taken {
            let both = format!("{} ({}).app", file_name(&program.name), program.id);
            folder.join(both)
        } else {
            named
        }
    }

    /// The folder `program`'s bundle goes in: the user's applications for an app, WaOS's own for
    /// a tool.
    fn folder_of(&self, program: &ProgramDescription) -> &Path {
        match program.kind {
            ProgramKind::App => &self.folders.installed,
            ProgramKind::Tool => &self.folders.tools,
        }
    }

    fn record(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the record and writes it whole.
    fn change_record(&self, change: impl FnOnce(&mut Record)) -> io::Result<()> {
        let mut record = self.record();
        change(&mut record);
        let text = toml::to_string(&*record).map_err(io::Error::other)?;
        if let Some(folder) = self.record_file.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(&self.record_file, text)
    }
}

/// The record at `file`; an empty one where there is none or it cannot be read, so every bundle is
/// written again as it is needed.
fn read_record(file: &Path) -> Record {
    let Ok(text) = fs::read_to_string(file) else {
        return Record::default();
    };
    toml::from_str(&text).unwrap_or_else(|error| {
        tracing::warn!(%error, file = %file.display(), "the record of the programs' apps is unreadable");
        Record::default()
    })
}

/// What a bundle made from `waos` is made from, as one value that changes whenever a bundle would
/// change: the SHA-256 of the loader it carries a copy of, in hex, then the paths it names, the
/// window library, the WaOS it starts and the data folder it looks WaOS up by. A WaOS updated in
/// place keeps the paths and changes the loader's hash; a WaOS run from elsewhere, another build of
/// it, changes the paths.
pub(crate) fn made_from(waos: &WaosFiles) -> io::Result<String> {
    let loader = Sha256::digest(fs::read(&waos.loader)?);
    let named = [&waos.window_library, &waos.waos, &waos.data_folder]
        .map(|path| path.to_string_lossy().into_owned());
    Ok(format!("{} {}", hex::encode(loader), named.join(" ")))
}

/// `name` as a file's name: a slash, which macOS's files cannot hold in a name, as a hyphen.
fn file_name(name: &str) -> String {
    name.replace('/', "-")
}

#[cfg(test)]
mod tests;
