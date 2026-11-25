//! Each installed program's entry where the system lists its apps, on Windows and Linux (decision
//! 24 of `docs/proposals/2026-11-13-what-six-apps-found.md`): a Start menu shortcut on Windows
//! (`start_menu`), a desktop entry on Linux (`desktop_entries`), each under the program's identity
//! (`logic::program_identities`) and starting `waos open --program <id>`. It is what finds the
//! program by its name, what its pin is, and on GNOME what its windows are matched to. An entry is
//! written when its program is installed or updated, deleted when it is removed, and the entries
//! are made to match the store as WaOS starts, for a program installed while an older WaOS ran. A
//! development run writes none: it shares the installed program's entry, and a program never
//! installed has none.
//!
//! WaOS keeps the record of the entries it wrote in its data folder: each one's files, and what
//! they were written from, its program's name, version and the WaOS it starts, so an entry is
//! written again only when one of those changed or one of its files is gone, and deleted with
//! every file it wrote.
//!
//! The invariant a later editor will break: the record names only files WaOS wrote, and every file
//! it wrote for an entry; an entry is recorded once its files are in place, a file it no longer
//! writes is deleted then, and an entry is taken out of the record only once its files are deleted.

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
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use waos_package::{Category, ProgramId};
use waos_store::Store;

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
    /// Its package's icon, a PNG, where it has one.
    pub(crate) icon: Option<PathBuf>,
    /// What it is for, where its author said, which a desktop's menus file it under.
    pub(crate) category: Option<Category>,
}

impl ProgramEntry {
    /// The entry of each program installed in `store` that has an identity.
    pub(crate) fn of_installed(store: &Store) -> Vec<ProgramEntry> {
        store
            .installed()
            .into_iter()
            .filter_map(|program| {
                let id = program.manifest.id().clone();
                Some(ProgramEntry {
                    app_id: app_id_of(&id)?,
                    name: program.name(),
                    version: program.manifest.version().to_string(),
                    icon: store.icon(&id),
                    category: program.manifest.category(),
                    id,
                })
            })
            .collect()
    }
}

/// How this system lists an app: the files that make a program's entry.
pub(crate) trait SystemEntries: Send + Sync {
    /// Writes the files of `entry`'s entry, which starts the WaOS `identities` names, none of them
    /// one of `others`, the files of the other programs' entries, and answers each file written.
    fn write(
        &self,
        entry: &ProgramEntry,
        identities: &Identities,
        others: &[PathBuf],
    ) -> io::Result<Vec<PathBuf>>;
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
    name: String,
    version: String,
    /// The WaOS it starts.
    waos: PathBuf,
}

/// Where the store's changes reach the entries' thread, which makes them match the store at each.
pub(crate) struct StoreChanges(Sender<()>);

impl StoreChanges {
    /// What is installed changed: a program was installed, updated or removed.
    pub(crate) fn changed(&self) {
        let _ = self.0.send(());
    }
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

    /// From now on, the entries match `store`'s installed programs, on a thread of their own: now,
    /// and again at each change told to what this answers.
    pub(crate) fn follow(self, store: Store) -> io::Result<StoreChanges> {
        let (changes, arriving) = mpsc::channel();
        std::thread::Builder::new()
            .name("waos-program-entries".to_owned())
            .spawn(move || self.keep_matching(&store, &arriving))?;
        let changes = StoreChanges(changes);
        changes.changed();
        Ok(changes)
    }

    /// Matches the store at each change arriving, those that came meanwhile as one, until no one
    /// can tell of another.
    fn keep_matching(&self, store: &Store, arriving: &Receiver<()>) {
        while arriving.recv().is_ok() {
            while arriving.try_recv().is_ok() {}
            self.match_installed(&ProgramEntry::of_installed(store));
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
                && kept.waos == self.identities.waos
                && kept.files.iter().all(|file| file.exists())
        })
    }

    /// Writes `entry`'s entry, deletes the files of its earlier one it no longer writes, and
    /// records it.
    fn write(&self, entry: &ProgramEntry) -> io::Result<()> {
        let others = self.files_of_others(&entry.id);
        let files = self.system.write(entry, &self.identities, &others)?;
        let earlier = self.record().entries.get(entry.id.as_str()).cloned();
        if let Some(earlier) = earlier {
            let stale = earlier.files.iter().filter(|file| !files.contains(file));
            delete_files(stale)?;
        }
        tracing::info!(program = %entry.id, ?files, "the program's entry is written");
        let kept = KeptEntry {
            files,
            name: entry.name.clone(),
            version: entry.version.clone(),
            waos: self.identities.waos.clone(),
        };
        self.change_record(|record| {
            record.entries.insert(entry.id.to_string(), kept);
        })
    }

    /// Deletes every file of the entry of the program `id`, then takes it out of the record.
    fn delete(&self, id: &str) -> io::Result<()> {
        let Some(kept) = self.record().entries.get(id).cloned() else {
            return Ok(());
        };
        delete_files(kept.files.iter())?;
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
