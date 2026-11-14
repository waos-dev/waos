use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// Entries that write one file each, named after the program's name, in `folder`, counting the
/// writes.
struct NamedFiles {
    folder: PathBuf,
    writes: Arc<AtomicUsize>,
}

impl SystemEntries for NamedFiles {
    fn write(
        &self,
        entry: &ProgramEntry,
        identities: &Identities,
        _others: &[PathBuf],
    ) -> io::Result<Vec<PathBuf>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let file = self.folder.join(format!("{}.entry", entry.name));
        fs::create_dir_all(&self.folder)?;
        fs::write(&file, identities.waos.to_string_lossy().as_bytes())?;
        Ok(vec![file])
    }
}

struct Entries {
    folder: tempfile::TempDir,
    writes: Arc<AtomicUsize>,
}

impl Entries {
    fn new() -> Entries {
        Entries {
            folder: tempfile::tempdir().unwrap(),
            writes: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// The entries as a WaOS at `waos` opens them.
    fn opened_by(&self, waos: &str) -> ProgramEntries {
        let system = NamedFiles {
            folder: self.folder.path().join("menu"),
            writes: Arc::clone(&self.writes),
        };
        let identities = Identities {
            waos: PathBuf::from(waos),
            icons: self.folder.path().join("icons"),
        };
        ProgramEntries::open(Box::new(system), identities, self.folder.path())
    }

    fn files(&self) -> Vec<String> {
        let Ok(read) = fs::read_dir(self.folder.path().join("menu")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = read
            .map(|file| file.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn writes(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }
}

fn entry(id: &str, name: &str, version: &str) -> ProgramEntry {
    let id: ProgramId = id.parse().unwrap();
    ProgramEntry {
        app_id: app_id_of(&id).unwrap(),
        id,
        name: name.to_owned(),
        version: version.to_owned(),
        icon: None,
    }
}

#[test]
fn each_installed_program_has_its_entry_written_once() {
    let entries = Entries::new();
    let kept = entries.opened_by("/usr/bin/waos");
    let installed = [
        entry("dev.example.notes", "Notes", "1.0.0"),
        entry("dev.example.clock", "Clock", "2.0.0"),
    ];
    kept.match_installed(&installed);
    assert_eq!(entries.files(), ["Clock.entry", "Notes.entry"]);
    assert_eq!(entries.writes(), 2);
    kept.match_installed(&installed);
    assert_eq!(
        entries.writes(),
        2,
        "nothing changed, so nothing written again"
    );
}

#[test]
fn an_update_writes_the_entry_again_and_a_rename_leaves_no_old_file() {
    let entries = Entries::new();
    let kept = entries.opened_by("/usr/bin/waos");
    kept.match_installed(&[entry("dev.example.notes", "Notes", "1.0.0")]);
    kept.match_installed(&[entry("dev.example.notes", "Notebook", "1.1.0")]);
    assert_eq!(entries.files(), ["Notebook.entry"]);
    assert_eq!(entries.writes(), 2);
}

#[test]
fn a_removed_program_loses_its_entry() {
    let entries = Entries::new();
    let kept = entries.opened_by("/usr/bin/waos");
    kept.match_installed(&[
        entry("dev.example.notes", "Notes", "1.0.0"),
        entry("dev.example.clock", "Clock", "2.0.0"),
    ]);
    kept.match_installed(&[entry("dev.example.clock", "Clock", "2.0.0")]);
    assert_eq!(entries.files(), ["Clock.entry"]);
}

#[test]
fn a_waos_started_later_matches_the_store_as_the_record_says_it_was_written() {
    let entries = Entries::new();
    entries.opened_by("/usr/bin/waos").match_installed(&[entry(
        "dev.example.notes",
        "Notes",
        "1.0.0",
    )]);
    let later = entries.opened_by("/usr/bin/waos");
    later.match_installed(&[
        entry("dev.example.notes", "Notes", "1.0.0"),
        entry("dev.example.clock", "Clock", "2.0.0"),
    ]);
    assert_eq!(entries.files(), ["Clock.entry", "Notes.entry"]);
    assert_eq!(
        entries.writes(),
        2,
        "the entry already written is left as it is"
    );
    later.match_installed(&[]);
    assert!(entries.files().is_empty(), "{:?}", entries.files());
}

#[test]
fn an_entry_whose_file_went_or_that_starts_another_waos_is_written_again() {
    let entries = Entries::new();
    let installed = [entry("dev.example.notes", "Notes", "1.0.0")];
    entries
        .opened_by("/usr/bin/waos")
        .match_installed(&installed);
    fs::remove_file(entries.folder.path().join("menu/Notes.entry")).unwrap();
    entries
        .opened_by("/usr/bin/waos")
        .match_installed(&installed);
    assert_eq!(entries.files(), ["Notes.entry"]);
    entries
        .opened_by("/opt/waos/waos")
        .match_installed(&installed);
    assert_eq!(entries.writes(), 3);
    let text = fs::read_to_string(entries.folder.path().join("menu/Notes.entry")).unwrap();
    assert_eq!(text, "/opt/waos/waos");
}
