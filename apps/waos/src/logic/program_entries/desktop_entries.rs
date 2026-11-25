//! A program's entry on Linux: the desktop entry `dev.waos.app.<id>.desktop` in the user's
//! applications (`$XDG_DATA_HOME/applications`), which the app grid and its search list, a pin in
//! the dock is, and GNOME and KDE match the program's windows to by their app id
//! (`StartupWMClass`); with the program's name, its icon by its path, a copy of its package's PNG
//! among WaOS's (`Identities::png_of`), and `Exec` starting `waos open --program <id>`. A
//! program that opens kinds of file names their MIME types (`MimeType`), as a Waft app's desktop
//! entry does, so the desktop offers it for them in Open with, and `Exec` then takes the files
//! (`%F`), opening them with the program; a kind without a MIME type is not associated, Linux
//! knowing files by theirs. The applications folder's cache of MIME types is updated after, where
//! the system has `update-desktop-database`, as a package's installation does.
//!
//! The invariants a later editor will break: the entry has `StartupNotify=true` from its first
//! write, since GNOME gives a launch from an entry it first read without it no activation token,
//! even once the key is added; and it is written beside its place and moved in, so the desktop,
//! which watches the folder, never reads half of one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use waos_package::Category;

use super::{ProgramEntry, SystemEntries, Written};
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
    ) -> io::Result<Written> {
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
        if !mime_types(entry).is_empty() {
            update_mime_cache(&self.applications);
        }
        Ok(Written {
            files,
            settings: Vec::new(),
        })
    }
}

/// The desktop entry of `entry`, starting the WaOS at `waos`, its icon at `icon`.
pub(crate) fn desktop_entry(entry: &ProgramEntry, waos: &Path, icon: Option<&Path>) -> String {
    let waos = waos.to_string_lossy();
    let mime_types = mime_types(entry);
    let mut exec: Vec<String> = std::iter::once(waos.to_string())
        .chain(relaunch_arguments(&entry.id))
        .map(|argument| exec_argument(&argument))
        .collect();
    if !mime_types.is_empty() {
        // The files opened, each its own argument, after `open` and before `--program`; none
        // when the entry is started alone.
        exec.insert(2, "%F".to_owned());
    }
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
    if let Some(category) = entry.category {
        lines.push(format!("Categories={}", menu_categories(category)));
    }
    if !mime_types.is_empty() {
        let listed: String = mime_types
            .iter()
            .map(|mime_type| format!("{};", string_value(mime_type)))
            .collect();
        lines.push(format!("MimeType={listed}"));
    }
    lines.push(format!("StartupWMClass={}", entry.app_id));
    lines.push("StartupNotify=true".to_owned());
    lines.join("\n") + "\n"
}

/// The MIME types of the kinds of file `entry`'s program opens, in the order declared, each once;
/// a kind without one is left out.
fn mime_types(entry: &ProgramEntry) -> Vec<&str> {
    let mut types: Vec<&str> = Vec::new();
    for mime_type in entry
        .file_associations
        .iter()
        .filter_map(|association| association.mime_type.as_deref())
    {
        if !types.contains(&mime_type) {
            types.push(mime_type);
        }
    }
    types
}

/// Updates the MIME types' cache of `applications`, which the desktop's Open with reads, where the
/// system has `update-desktop-database`; a system without it reads the entries themselves.
fn update_mime_cache(applications: &Path) {
    match Command::new("update-desktop-database")
        .arg(applications)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(%status, "update-desktop-database failed"),
        Err(error) => tracing::debug!(%error, "no update-desktop-database to run"),
    }
}

/// The desktop menu's categories `category` is filed under, as `Categories` lists them: the
/// freedesktop menu specification's main categories, which every desktop's menus know, and with
/// Audio or Video the AudioVideo the specification asks beside them. They are coarser than the
/// manifest's, so several of those share one.
fn menu_categories(category: Category) -> &'static str {
    match category {
        Category::Business | Category::Finance | Category::Productivity => "Office;",
        Category::DeveloperTools => "Development;",
        Category::Education | Category::Reference => "Education;",
        Category::Entertainment => "AudioVideo;",
        Category::Games => "Game;",
        Category::GraphicsDesign | Category::Photography => "Graphics;",
        Category::Medical => "Science;",
        Category::Music => "AudioVideo;Audio;",
        Category::News | Category::SocialNetworking => "Network;",
        Category::Video => "AudioVideo;Video;",
        Category::HealthcareFitness
        | Category::Lifestyle
        | Category::Sports
        | Category::Travel
        | Category::Utilities
        | Category::Weather => "Utility;",
    }
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
    use waos_package::FileAssociation;

    use super::*;

    fn entry(name: &str, icon: Option<PathBuf>) -> ProgramEntry {
        ProgramEntry {
            id: "dev.example.notes".parse().unwrap(),
            app_id: "dev.waos.app.dev.example.notes".to_owned(),
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            icon,
            category: None,
            file_associations: Vec::new(),
        }
    }

    #[test]
    fn an_app_filed_under_a_category_is_under_the_menus_own_and_one_filed_under_none_says_none() {
        let waos = Path::new("/opt/waos/waos");
        let filed = |category| ProgramEntry {
            category,
            ..entry("Notes", None)
        };
        let music = desktop_entry(&filed(Some(Category::Music)), waos, None);
        assert!(music.contains("Categories=AudioVideo;Audio;\n"), "{music}");
        let tools = desktop_entry(&filed(Some(Category::DeveloperTools)), waos, None);
        assert!(tools.contains("Categories=Development;\n"), "{tools}");
        assert!(!desktop_entry(&filed(None), waos, None).contains("Categories="));
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
            .unwrap()
            .files;
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

    #[test]
    fn a_program_that_opens_kinds_of_file_names_their_mime_types_and_takes_the_files() {
        let flash = FileAssociation {
            extensions: vec!["swf".to_owned()],
            name: "Flash movie".to_owned(),
            mime_type: Some("application/x-shockwave-flash".to_owned()),
        };
        let unnamed = FileAssociation {
            extensions: vec!["spl".to_owned()],
            name: "FutureSplash movie".to_owned(),
            mime_type: None,
        };
        let opener = ProgramEntry {
            file_associations: vec![flash.clone(), unnamed, flash],
            ..entry("Ruffle", None)
        };
        let text = desktop_entry(&opener, Path::new("/usr/bin/waos"), None);
        assert!(
            text.contains("Exec=/usr/bin/waos open %F --program dev.example.notes\n"),
            "{text}"
        );
        assert!(
            text.contains("MimeType=application/x-shockwave-flash;\n"),
            "{text}"
        );
        let plain = desktop_entry(&entry("Notes", None), Path::new("/usr/bin/waos"), None);
        assert!(!plain.contains("MimeType="), "{plain}");
        assert!(!plain.contains("%F"), "{plain}");
    }
}
