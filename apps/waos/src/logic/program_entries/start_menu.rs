//! A program's entry on Windows: a shortcut in the Start menu's `WaOS Programs` folder, named after
//! the program, which Start's search finds by that name, carrying the program's identity as its
//! AppUserModelID, so a pin of the program's taskbar button is that shortcut; it starts
//! `waos.exe open --program <id>`, and its icon is the program's `.ico` among WaOS's
//! (`Identities::ico_of`), written from its package's PNG (`ico`).
//!
//! The invariant a later editor will break: the shortcut carries its AppUserModelID from the moment
//! it is written, as Microsoft asks, and two programs of the same name never share a file: the one
//! written second takes its id in its name.

use std::io;
use std::path::{Path, PathBuf};

use super::{ProgramEntry, SystemEntries};
use crate::logic::program_identities::{Identities, relaunch_arguments, windows_arguments};

/// What a program's shortcut holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shortcut {
    /// Where it goes, its file's name the program's.
    pub(crate) path: PathBuf,
    /// The WaOS it starts.
    pub(crate) target: PathBuf,
    /// What WaOS is started with.
    pub(crate) arguments: String,
    /// The program's identity.
    pub(crate) app_user_model_id: String,
    /// The program's `.ico`, its icon at index 0; WaOS's own where there is none.
    pub(crate) icon: Option<PathBuf>,
    /// What Start shows under its name: the program's name.
    pub(crate) description: String,
}

/// The shortcuts in `folder`, the Start menu's folder of WaOS's programs.
pub(crate) struct StartMenu {
    pub(crate) folder: PathBuf,
}

impl SystemEntries for StartMenu {
    fn write(
        &self,
        entry: &ProgramEntry,
        identities: &Identities,
        others: &[PathBuf],
    ) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let icon = match &entry.icon {
            Some(png) => {
                let ico = identities.ico_of(&entry.app_id);
                let picture = std::fs::read(png)?;
                let written = super::ico::ico_of(&picture).map_err(io::Error::other)?;
                write_file(&ico, &written)?;
                files.push(ico.clone());
                Some(ico)
            }
            None => None,
        };
        let shortcut = shortcut_of(entry, identities, &self.folder, others, icon);
        write_shortcut(&shortcut)?;
        files.push(shortcut.path);
        Ok(files)
    }
}

/// The shortcut of `entry` in `folder`, starting the WaOS `identities` names, its icon `icon`,
/// named after the program unless `others`, the other entries' files, hold that name.
pub(crate) fn shortcut_of(
    entry: &ProgramEntry,
    identities: &Identities,
    folder: &Path,
    others: &[PathBuf],
    icon: Option<PathBuf>,
) -> Shortcut {
    let named = folder.join(format!("{}.lnk", file_name_of(&entry.name, &entry.app_id)));
    let path = if others.contains(&named) {
        let both = format!("{} ({})", entry.name, entry.id);
        folder.join(format!("{}.lnk", file_name_of(&both, &entry.app_id)))
    } else {
        named
    };
    Shortcut {
        path,
        target: identities.waos.clone(),
        arguments: windows_arguments(&relaunch_arguments(&entry.id)),
        app_user_model_id: entry.app_id.clone(),
        icon,
        description: entry.name.clone(),
    }
}

/// `name` as a file's name Windows takes: the characters it refuses as `_`, without the dots and
/// spaces it drops from the end; `fallback` where nothing of it is left, or it is a device's name.
fn file_name_of(name: &str, fallback: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            control if control.is_control() => '_',
            other => other,
        })
        .collect();
    let trimmed = replaced.trim().trim_end_matches(['.', ' ']);
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|last: char| last.is_ascii_digit()));
    if trimmed.is_empty() || device {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(path, bytes)
}

#[cfg(windows)]
fn write_shortcut(shortcut: &Shortcut) -> io::Result<()> {
    if let Some(folder) = shortcut.path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    crate::os::write_shortcut(shortcut)
}

/// Shortcuts are written on Windows alone; the tests elsewhere write none.
#[cfg(not(windows))]
fn write_shortcut(_shortcut: &Shortcut) -> io::Result<()> {
    Err(io::Error::other("a Start menu shortcut is Windows' alone"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> ProgramEntry {
        ProgramEntry {
            id: "dev.example.notes".parse().unwrap(),
            app_id: "dev.waos.app.dev.example.notes".to_owned(),
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            icon: None,
        }
    }

    fn identities() -> Identities {
        Identities {
            waos: PathBuf::from(r"C:\Program Files\WaOS\waos.exe"),
            icons: PathBuf::from(r"C:\Users\me\AppData\Local\WaOS\icons"),
        }
    }

    #[test]
    fn the_shortcut_is_named_after_the_program_and_carries_its_identity() {
        let folder = Path::new(r"C:\Start\WaOS Programs");
        let ico = PathBuf::from(r"C:\Users\me\AppData\Local\WaOS\icons\notes.ico");
        let shortcut = shortcut_of(
            &entry("Notes"),
            &identities(),
            folder,
            &[],
            Some(ico.clone()),
        );
        assert_eq!(
            shortcut,
            Shortcut {
                path: folder.join("Notes.lnk"),
                target: PathBuf::from(r"C:\Program Files\WaOS\waos.exe"),
                arguments: "open --program dev.example.notes".to_owned(),
                app_user_model_id: "dev.waos.app.dev.example.notes".to_owned(),
                icon: Some(ico),
                description: "Notes".to_owned(),
            }
        );
    }

    #[test]
    fn a_second_program_of_the_same_name_takes_its_id_in_its_shortcuts_name() {
        let folder = Path::new(r"C:\Start\WaOS Programs");
        let taken = [folder.join("Notes.lnk")];
        let shortcut = shortcut_of(&entry("Notes"), &identities(), folder, &taken, None);
        assert_eq!(shortcut.path, folder.join("Notes (dev.example.notes).lnk"));
    }

    #[test]
    fn a_name_windows_refuses_as_a_files_is_made_one_it_takes() {
        let fallback = "dev.waos.app.dev.example.notes";
        assert_eq!(file_name_of("A/B: C?", fallback), "A_B_ C_");
        assert_eq!(file_name_of("Notes. ", fallback), "Notes");
        assert_eq!(file_name_of("...", fallback), fallback);
        assert_eq!(file_name_of("con", fallback), fallback);
        assert_eq!(file_name_of("COM1.txt", fallback), fallback);
        assert_eq!(file_name_of("Comet", fallback), "Comet");
    }
}
