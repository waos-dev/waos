//! A program's entry on Linux: the desktop entry `dev.waos.app.<id>.desktop` in the user's
//! applications (`$XDG_DATA_HOME/applications`), which the app grid and its search list, a pin in
//! the dock is, and GNOME and KDE match the program's windows to by their app id
//! (`StartupWMClass`); with the program's name, its icon by its path, a copy of its package's PNG
//! among WaOS's (`Identities::png_of`), and `Exec` starting `waos open --program <id>`
//! (`../experiments/2026-11-14-window-app-identity`).
//!
//! The invariants a later editor will break: the entry has `StartupNotify=true` from its first
//! write, since GNOME gives a launch from an entry it first read without it no activation token,
//! even once the key is added; and it is written beside its place and moved in, so the desktop,
//! which watches the folder, never reads half of one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{ProgramEntry, SystemEntries};
use crate::logic::program_identities::{Identities, relaunch_arguments};

/// The desktop entries in `applications`, the user's applications folder.
pub(crate) struct DesktopEntries {
    pub(crate) applications: PathBuf,
}

impl SystemEntries for DesktopEntries {
    /// Writes a desktop entry named by the program's identity, which no other program's has.
    fn write(
        &self,
        entry: &ProgramEntry,
        identities: &Identities,
        _others: &[PathBuf],
    ) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let icon = match &entry.icon {
            Some(png) => {
                let copy = identities.png_of(&entry.app_id);
                write_in_place(&copy, &fs::read(png)?)?;
                files.push(copy.clone());
                Some(copy)
            }
            None => None,
        };
        let path = self.applications.join(format!("{}.desktop", entry.app_id));
        let text = desktop_entry(entry, &identities.waos, icon.as_deref());
        write_in_place(&path, text.as_bytes())?;
        files.push(path);
        Ok(files)
    }
}

/// The desktop entry of `entry`, starting the WaOS at `waos`, its icon at `icon`.
pub(crate) fn desktop_entry(entry: &ProgramEntry, waos: &Path, icon: Option<&Path>) -> String {
    let waos = waos.to_string_lossy();
    let exec: Vec<String> = std::iter::once(waos.to_string())
        .chain(relaunch_arguments(&entry.id))
        .map(|argument| exec_argument(&argument))
        .collect();
    let mut lines = vec![
        "[Desktop Entry]".to_owned(),
        "Type=Application".to_owned(),
        format!("Name={}", string_value(&entry.name)),
        format!("Exec={}", string_value(&exec.join(" "))),
        format!("TryExec={}", string_value(&waos)),
    ];
    if let Some(icon) = icon {
        lines.push(format!("Icon={}", string_value(&icon.to_string_lossy())));
    }
    lines.push(format!("StartupWMClass={}", entry.app_id));
    lines.push("StartupNotify=true".to_owned());
    lines.join("\n") + "\n"
}

/// `argument` as `Exec` holds it: bare where it has none of the characters the specification
/// reserves, else in double quotes, its quotes, backticks, dollars and backslashes escaped; a
/// `%`, which starts a field code, doubled either way.
fn exec_argument(argument: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let quoted = if argument.is_empty() || argument.contains(RESERVED) {
        let mut quoted = String::from('"');
        for character in argument.chars() {
            if matches!(character, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        quoted
    } else {
        argument.to_owned()
    };
    quoted.replace('%', "%%")
}

/// `text` as a value of type string holds it: its backslashes, line ends, tabs and returns escaped,
/// and a space it starts with, which a reader would trim.
fn string_value(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r");
    match escaped.strip_prefix(' ') {
        Some(rest) => format!("\\s{rest}"),
        None => escaped,
    }
}

/// Writes `bytes` to `path`, beside it first and then moved in, its folder made where it has none.
fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("a file with no folder"))?;
    fs::create_dir_all(folder)?;
    let mut beside = tempfile::NamedTempFile::new_in(folder)?;
    io::Write::write_all(&mut beside, bytes)?;
    beside.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, icon: Option<PathBuf>) -> ProgramEntry {
        ProgramEntry {
            id: "dev.example.notes".parse().unwrap(),
            app_id: "dev.waos.app.dev.example.notes".to_owned(),
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            icon,
        }
    }

    #[test]
    fn the_entry_names_the_program_starts_waos_for_it_and_matches_its_windows() {
        let text = desktop_entry(
            &entry("Notes", None),
            Path::new("/usr/bin/waos"),
            Some(Path::new(
                "/home/me/.local/share/waos/icons/dev.waos.app.dev.example.notes.png",
            )),
        );
        assert_eq!(
            text,
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Notes\n\
             Exec=/usr/bin/waos open --program dev.example.notes\n\
             TryExec=/usr/bin/waos\n\
             Icon=/home/me/.local/share/waos/icons/dev.waos.app.dev.example.notes.png\n\
             StartupWMClass=dev.waos.app.dev.example.notes\n\
             StartupNotify=true\n"
        );
    }

    #[test]
    fn a_waos_at_an_awkward_path_is_quoted_and_escaped_as_the_specification_says() {
        let text = desktop_entry(
            &entry(" Notes\nand more", None),
            Path::new("/opt/Wa OS/100%/a\\b/waos"),
            None,
        );
        assert!(text.contains("Name=\\sNotes\\nand more\n"), "{text}");
        assert!(
            text.contains(
                "Exec=\"/opt/Wa OS/100%%/a\\\\\\\\b/waos\" open --program dev.example.notes\n"
            ),
            "{text}"
        );
        assert!(!text.contains("Icon="), "{text}");
    }

    #[test]
    fn writing_an_entry_puts_it_and_a_copy_of_its_icon_in_place() {
        let folder = tempfile::tempdir().unwrap();
        let png = folder.path().join("package-icon.png");
        fs::write(&png, b"a picture").unwrap();
        let entries = DesktopEntries {
            applications: folder.path().join("applications"),
        };
        let identities = Identities {
            waos: PathBuf::from("/usr/bin/waos"),
            icons: folder.path().join("icons"),
        };
        let files = entries
            .write(&entry("Notes", Some(png)), &identities, &[])
            .unwrap();
        let copy = folder
            .path()
            .join("icons/dev.waos.app.dev.example.notes.png");
        let desktop = folder
            .path()
            .join("applications/dev.waos.app.dev.example.notes.desktop");
        assert_eq!(files, [copy.clone(), desktop.clone()]);
        assert_eq!(fs::read(&copy).unwrap(), b"a picture");
        let text = fs::read_to_string(&desktop).unwrap();
        let icon = string_value(&files[0].to_string_lossy());
        assert!(text.contains(&format!("Icon={icon}\n")), "{text}");
        let leftovers = fs::read_dir(folder.path().join("applications"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 1, "nothing beside the entry");
    }
}
