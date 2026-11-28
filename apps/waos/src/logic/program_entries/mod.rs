//! Each installed program's entry where the system lists its apps, on Windows and Linux: a Start
//! menu shortcut on Windows (`start_menu`), a desktop entry on Linux (`desktop_entries`), each
//! under the program's identity (`logic::program_identities`) and starting `waos open --program
//! <id>`. It is what finds the program by its name, what its pin is, and on GNOME what its windows
//! are matched to. An entry is written when its program is installed or updated, deleted when it is
//! removed, and the entries are made to match the store as WaOS starts, for a program installed
//! while an older WaOS ran. A development run writes none: it shares the installed program's entry,
//! and a program never installed has none. An entry also offers its program for the kinds of file
//! it opens: a desktop entry names their MIME types, and on Windows they are registered in the
//! user's classes (`file_associations`).
//!
//! WaOS keeps the record of the entries it wrote in its data folder: each one's files and what it
//! set in the registry, and what they were written from, its program's name, version and the WaOS
//! it starts, so an entry is written again only when one of those changed or one of its files is
//! gone, and deleted with every file it wrote and everything it set.
//!
//! The invariant a later editor will break: the record names only files WaOS wrote and registry
//! settings it made, and every one it made for an entry; an entry is recorded once they are in
//! place, one it no longer makes is taken back then, and an entry is taken out of the record only
//! once they are all taken back.

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "tested on every desktop, written on Linux")
)]
pub(crate) mod desktop_entries;
#[cfg(any(windows, test))]
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "tested on every desktop, written on Windows")
)]
pub(crate) mod file_associations;
#[cfg(any(windows, test))]
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "tested on every desktop, written on Windows")
)]
pub(crate) mod ico;
#[cfg(any(windows, test))]
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "tested on every desktop, written on Windows")
)]
pub(crate) mod start_menu;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use waos_package::{Category, FileAssociation, ProgramId};
use waos_services::packages::PackageManager;

#[cfg(any(windows, target_os = "linux"))]
use super::folders::AppFolders;
use super::program_identities::{Identities, app_id_of};

/// The record's file in WaOS's data folder.
const RECORD: &str = "program-entries.toml";

/// What a program's entry is written from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgramEntry {
    pub(crate) id: ProgramId,
    /// Its identity, `dev.waos.app.<id>`.
    pub(crate) app_id: String,
    /// Its name as its user knows it.
    pub(crate) name: String,
    pub(crate) version: String,
    /// Its package's icon, a PNG's bytes, where it has one.
    pub(crate) icon: Option<Arc<[u8]>>,
    /// What it is for, where its author said, which a desktop's menus file it under.
    pub(crate) category: Option<Category>,
    /// The kinds of file it opens, which the system offers it for.
    pub(crate) file_associations: Vec<FileAssociation>,
}

impl ProgramEntry {
    /// The entry of each program installed in `packages` that has an identity.
    pub(crate) fn of_installed(packages: &PackageManager) -> Vec<ProgramEntry> {
        packages
            .installed()
            .into_iter()
            .filter_map(|program| {
                let id = program.manifest.id().clone();
                Some(ProgramEntry {
                    app_id: app_id_of(&id)?,
                    name: program.name(),
                    version: program.manifest.version().to_string(),
                    icon: packages.icon(&id),
                    category: program.manifest.category(),
                    file_associations: program.manifest.file_associations().to_vec(),
                    id,
                })
            })
            .collect()
    }
}

/// How this system lists an app: the files, and on Windows the registry settings, that make a
/// program's entry.
pub(crate) trait SystemEntries: Send + Sync {
    /// Writes `entry`'s entry, which starts the WaOS `identities` names, none of its files one of
    /// `others`, the files of the other programs' entries, and answers what it wrote.
    fn write(
        &self,
        entry: &ProgramEntry,
        identities: &Identities,
        others: &[PathBuf],
    ) -> io::Result<Written>;

    /// Takes back `settings`, registry settings an entry made; nothing on a system whose entries
    /// make none.
    fn take_back(&self, settings: &[RegistrySetting]) -> io::Result<()> {
        debug_assert!(settings.is_empty(), "only Windows' entries make settings");
        Ok(())
    }
}

/// What an entry's writing made: its files, and its registry settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Written {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) settings: Vec<RegistrySetting>,
}

/// A registry setting an entry made under `HKEY_CURRENT_USER`, as it is taken back: a key of its
/// own, deleted whole, or one value of its own in a key it shares with other apps, deleted alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RegistrySetting {
    Key(String),
    Value { key: String, name: String },
}

/// The entries WaOS wrote, and the record of them.
pub(crate) struct ProgramEntries {
    system: Box<dyn SystemEntries>,
    identities: Identities,
    record_file: PathBuf,
    record: Mutex<Record>,
}

/// What WaOS keeps of the entries it wrote, by their programs' ids.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Record {
    entries: BTreeMap<String, KeptEntry>,
}

/// One entry WaOS wrote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct KeptEntry {
    /// Every file written for it.
    files: Vec<PathBuf>,
    /// Every registry setting made for it; none for an entry recorded before WaOS made any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    settings: Vec<RegistrySetting>,
    name: String,
    version: String,
    /// The kinds of file it was written to open; none for an entry recorded before WaOS kept
    /// them, which opened none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    file_associations: Vec<FileAssociation>,
    /// The WaOS it starts.
    waos: PathBuf,
}

impl ProgramEntries {
    /// The entries of this system's own kind, among `folders`, starting the WaOS `identities`
    /// names: Start menu shortcuts on Windows, desktop entries on Linux.
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn of_this_system(folders: &AppFolders, identities: Identities) -> ProgramEntries {
        #[cfg(windows)]
        let system = start_menu::StartMenu {
            folder: folders.start_menu.clone(),
        };
        #[cfg(target_os = "linux")]
        let system = desktop_entries::DesktopEntries {
            applications: folders.applications.clone(),
        };
        ProgramEntries::open(Box::new(system), identities, &folders.data)
    }

    /// From now on, the entries match the installed programs of `packages`, on a thread of their
    /// own: now, and again at each change the package manager announces.
    pub(crate) fn follow(self, packages: Arc<PackageManager>) -> io::Result<()> {
        std::thread::Builder::new()
            .name("waos-program-entries".to_owned())
            .spawn(move || self.keep_matching(&packages))?;
        Ok(())
    }

    /// Matches the store now and at each change it announces, those that came meanwhile as one,
    /// until the package manager is gone.
    fn keep_matching(&self, packages: &PackageManager) {
        let mut changes = packages.installed_changes();
        loop {
            changes.borrow_and_update();
            self.match_installed(&ProgramEntry::of_installed(packages));
            if futures::executor::block_on(changes.changed()).is_err() {
                return;
            }
        }
    }

    /// The entries `system` writes, starting the WaOS `identities` names, as the record in
    /// `data_folder` says they were written.
    pub(crate) fn open(
        system: Box<dyn SystemEntries>,
        identities: Identities,
        data_folder: &Path,
    ) -> ProgramEntries {
        let record_file = data_folder.join(RECORD);
        let record = read_record(&record_file);
        ProgramEntries {
            system,
            identities,
            record_file,
            record: Mutex::new(record),
        }
    }

    /// Makes the entries match `installed`, the installed programs: each has its entry, as it is
    /// now, and a program no longer installed has none. A failure is said, and the next match
    /// tries again.
    pub(crate) fn match_installed(&self, installed: &[ProgramEntry]) {
        let kept: Vec<String> = self.record().entries.keys().cloned().collect();
        for id in kept {
            if !installed.iter().any(|entry| entry.id.as_str() == id) {
                log_failure(&id, self.delete(&id));
            }
        }
        for entry in installed {
            if !self.is_current(entry) {
                log_failure(entry.id.as_str(), self.write(entry));
            }
        }
    }

    /// Whether `entry`'s entry is written as it is now, with every file in place.
    fn is_current(&self, entry: &ProgramEntry) -> bool {
        let record = self.record();
        record.entries.get(entry.id.as_str()).is_some_and(|kept| {
            kept.name == entry.name
                && kept.version == entry.version
                && kept.file_associations == entry.file_associations
                && kept.waos == self.identities.waos
                && kept.files.iter().all(|file| file.exists())
        })
    }

    /// Writes `entry`'s entry, takes back what its earlier one made that it no longer makes, and
    /// records it.
    fn write(&self, entry: &ProgramEntry) -> io::Result<()> {
        let others = self.files_of_others(&entry.id);
        let Written { files, settings } = self.system.write(entry, &self.identities, &others)?;
        let earlier = self.record().entries.get(entry.id.as_str()).cloned();
        if let Some(earlier) = earlier {
            let stale = earlier.files.iter().filter(|file| !files.contains(file));
            delete_files(stale)?;
            let stale: Vec<RegistrySetting> = earlier
                .settings
                .into_iter()
                .filter(|setting| !settings.contains(setting))
                .collect();
            self.system.take_back(&stale)?;
        }
        tracing::info!(program = %entry.id, ?files, ?settings, "the program's entry is written");
        let kept = KeptEntry {
            files,
            settings,
            name: entry.name.clone(),
            version: entry.version.clone(),
            file_associations: entry.file_associations.clone(),
            waos: self.identities.waos.clone(),
        };
        self.change_record(|record| {
            record.entries.insert(entry.id.to_string(), kept);
        })
    }

    /// Deletes every file of the entry of the program `id` and takes back its registry settings,
    /// then takes it out of the record.
    fn delete(&self, id: &str) -> io::Result<()> {
        let Some(kept) = self.record().entries.get(id).cloned() else {
            return Ok(());
        };
        delete_files(kept.files.iter())?;
        self.system.take_back(&kept.settings)?;
        tracing::info!(program = id, "the program's entry is deleted");
        self.change_record(|record| {
            record.entries.remove(id);
        })
    }

    /// The files of every entry but the program `id`'s.
    fn files_of_others(&self, id: &ProgramId) -> Vec<PathBuf> {
        let record = self.record();
        let others = record
            .entries
            .iter()
            .filter(|(kept, _)| kept.as_str() != id.as_str());
        others.flat_map(|(_, kept)| kept.files.clone()).collect()
    }

    fn record(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the record as `change` does, and writes it.
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

/// Deletes each of `files`; one already gone is no failure.
fn delete_files<'a>(files: impl Iterator<Item = &'a PathBuf>) -> io::Result<()> {
    for file in files {
        match fs::remove_file(file) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// The record in `file`; an empty one where there is none, or it cannot be read, which is said.
fn read_record(file: &Path) -> Record {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Record::default(),
        Err(error) => {
            tracing::warn!(file = %file.display(), %error, "the record of the programs' entries cannot be read");
            return Record::default();
        }
    };
    toml::from_str(&text).unwrap_or_else(|error| {
        tracing::warn!(file = %file.display(), %error, "the record of the programs' entries cannot be read");
        Record::default()
    })
}

fn log_failure(program: &str, result: io::Result<()>) {
    if let Err(error) = result {
        tracing::warn!(program, %error, "the program's entry could not be kept");
    }
}

#[cfg(test)]
mod tests;
