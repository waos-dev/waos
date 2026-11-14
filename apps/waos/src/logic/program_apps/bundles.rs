//! Each program's app bundle as WaOS keeps it, as Chrome keeps its app shims: an installed
//! program's in a folder of the user's applications, named after the program, with its package's
//! name, version and icon; a program run in development's under WaOS's caches, named by its id,
//! the id its only name. Each is written at install, or at the first launch that finds none,
//! rewritten when the program is updated, and deleted when it is removed. WaOS keeps, in its data
//! folder, where each bundle is and the code hash it was signed with, which its process is checked
//! against at each connection, and the system's version it was signed on: when the system's
//! version changes, every bundle is rebuilt, since Gatekeeper then treats the ad-hoc signatures
//! made before as unverified (Chrome's commit `91dc901582`).
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
use std::sync::{Mutex, PoisonError};

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use waos_package::ProgramId;

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
    /// Its package's icon; none for a program run in development.
    pub(crate) icon: Option<PathBuf>,
    /// Whether it is installed, rather than run in development.
    pub(crate) installed: bool,
}

/// Where the bundles go.
#[derive(Clone, Debug)]
pub(crate) struct BundleFolders {
    /// Installed programs': a folder of the user's applications.
    pub(crate) installed: PathBuf,
    /// Programs run in development: a folder of WaOS's caches.
    pub(crate) development: PathBuf,
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
pub(crate) struct Bundles {
    folders: BundleFolders,
    waos: WaosFiles,
    /// The key each code hash's HMAC is made under.
    key: Vec<u8>,
    record_file: PathBuf,
    record: Mutex<Record>,
    /// The system's version now.
    system: String,
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
    /// Whether it is an installed program's, rather than one run in development's.
    installed: bool,
}

impl Bundles {
    /// The bundles in `folders`, referring to `waos`'s files, as the record in `data_folder` says
    /// they were written, on the system at `system`, their code hashes vouched for under `key`.
    pub(crate) fn open(
        folders: BundleFolders,
        waos: WaosFiles,
        data_folder: &Path,
        system: String,
        key: Vec<u8>,
    ) -> Bundles {
        let record_file = data_folder.join(RECORD);
        let record = read_record(&record_file);
        Bundles {
            folders,
            waos,
            key,
            record_file,
            record: Mutex::new(record),
            system,
        }
    }

    /// Whether the bundles were signed on another version of the system than this one.
    pub(crate) fn signed_on_another_system(&self) -> bool {
        let record = self.record();
        !record.apps.is_empty() && record.system != self.system
    }

    /// The programs that have a bundle.
    pub(crate) fn programs(&self) -> Vec<ProgramId> {
        self.record()
            .apps
            .keys()
            .filter_map(|id| id.parse().ok())
            .collect()
    }

    /// The installed programs that have a bundle.
    pub(crate) fn installed_programs(&self) -> Vec<ProgramId> {
        self.record()
            .apps
            .iter()
            .filter(|(_, kept)| kept.installed)
            .filter_map(|(id, _)| id.parse().ok())
            .collect()
    }

    /// The bundle of `program`, as WaOS wrote it, at the version it is now; written now if there
    /// is none, or it is of another version.
    pub(crate) fn ensure(&self, program: &ProgramDescription) -> io::Result<KeptBundle> {
        let kept = self.kept(&program.id);
        match kept {
            Some(kept)
                if kept.version == program.version
                    && kept.installed == program.installed
                    && kept.path.exists() =>
            {
                Ok(kept)
            }
            _ => self.write(program),
        }
    }

    /// Writes the bundle of `program` again, in place of the one there.
    pub(crate) fn write(&self, program: &ProgramDescription) -> io::Result<KeptBundle> {
        let path = self.place_of(program);
        let contents = BundleContents {
            program: &program.id,
            name: &program.name,
            version: &program.version,
            icon: program.icon.as_deref(),
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
            installed: program.installed,
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

    /// Where `program`'s bundle goes: named after it in the installed programs' folder, unless
    /// another program's bundle has that name, when its id is added; by its id in development's.
    fn place_of(&self, program: &ProgramDescription) -> PathBuf {
        if !program.installed {
            return self.folders.development.join(format!("{}.app", program.id));
        }
        let named = self
            .folders
            .installed
            .join(format!("{}.app", file_name(&program.name)));
        let taken = self
            .record()
            .apps
            .iter()
            .any(|(id, kept)| kept.path == named && id != program.id.as_str());
        if taken {
            let both = format!("{} ({}).app", file_name(&program.name), program.id);
            self.folders.installed.join(both)
        } else {
            named
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

/// `name` as a file's name: a slash, which macOS's files cannot hold in a name, as a hyphen.
fn file_name(name: &str) -> String {
    name.replace('/', "-")
}

#[cfg(test)]
mod tests;
