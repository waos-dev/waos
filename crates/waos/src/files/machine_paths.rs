//! How a program names a path of this computer, and back. On Unix's systems a path is the same in
//! the program as on the machine. On Windows a drive's path, `C:\Users\me`, is `/c/Users/me`, as
//! MSYS2 and Git's shell name it: the drive's letter lowercase, its names between `/`; and `/`
//! itself is the folder of the drives, which only a machine's root shows (the lead's decision of
//! 2026-11-13, on `windows-files-design.md`). A path Windows names otherwise, a share's
//! (`\\server\share`) or a device's, has no name in a program.
//!
//! The invariant: every place the engine gives a program a path of this computer, or takes one
//! from it, names it through here: a file handed at its own path, a machine's root and what it
//! opens, a symlink a machine's path holds, and a command's folder.

use std::path::{Path, PathBuf};

/// A machine's root as this computer gives it, `/`: on Windows the folder of its drives.
pub const ROOT: &str = "/";

/// Where `path`, a path of this computer, is as a program names it; none where it has no name in a
/// program.
#[cfg(unix)]
pub fn in_program(path: &Path) -> Option<PathBuf> {
    path.is_absolute().then(|| path.to_path_buf())
}

/// Where `path`, a program's absolute path, is on this computer; none where it names no path of it:
/// on Windows `/` and a path whose first name is no drive's letter.
#[cfg(unix)]
pub fn on_this_computer(path: &Path) -> Option<PathBuf> {
    path.is_absolute().then(|| path.to_path_buf())
}

#[cfg(windows)]
pub fn in_program(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let letter = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return None,
        },
        _ => return None,
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let mut named = PathBuf::from(format!("/{}", char::from(letter).to_ascii_lowercase()));
    for component in components {
        match component {
            Component::Normal(name) => named.push(name),
            Component::ParentDir => {
                named.push("..");
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Some(PathBuf::from(named.to_string_lossy().replace('\\', "/")))
}

#[cfg(windows)]
pub fn on_this_computer(path: &Path) -> Option<PathBuf> {
    let text = path.to_str()?.replace('\\', "/");
    let rest = text.strip_prefix('/')?;
    let (letter, names) = rest.split_once('/').unwrap_or((rest, ""));
    let mut letters = letter.chars();
    let (Some(letter), None) = (letters.next(), letters.next()) else {
        return None;
    };
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let drive = format!("{}:\\", letter.to_ascii_uppercase());
    Some(Path::new(&drive).join(names.replace('/', "\\")))
}

/// `path`, a path of this computer as the system resolved it, as people and other apps name it:
/// on Windows a drive's path without the `\\?\` that resolving puts before it, which a file URL
/// cannot hold.
#[cfg(unix)]
pub fn plain(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
pub fn plain(path: PathBuf) -> PathBuf {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let Prefix::VerbatimDisk(letter) = prefix.kind() else {
        return path;
    };
    let mut plain = PathBuf::from(format!("{}:\\", char::from(letter)));
    plain.extend(components.filter(|component| *component != Component::RootDir));
    plain
}

/// The drives of this computer, by the names a program gives them: `c`, `d`.
#[cfg(windows)]
pub(crate) fn drives() -> Vec<String> {
    (b'a'..=b'z')
        .map(char::from)
        .filter(|letter| Path::new(&format!("{}:\\", letter.to_ascii_uppercase())).is_dir())
        .map(String::from)
        .collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_drive_s_path_is_named_from_its_lowercase_letter_and_back() {
        let pairs = [
            (r"C:\Users\me\notes.txt", "/c/Users/me/notes.txt"),
            (r"D:\", "/d"),
            (r"\\?\C:\Program Files", "/c/Program Files"),
        ];
        for (machine, program) in pairs {
            assert_eq!(in_program(Path::new(machine)), Some(PathBuf::from(program)));
        }
        assert_eq!(
            on_this_computer(Path::new("/c/Users/me/notes.txt")),
            Some(PathBuf::from(r"C:\Users\me\notes.txt"))
        );
        assert_eq!(
            on_this_computer(Path::new("/d")),
            Some(PathBuf::from(r"D:\"))
        );
    }

    #[test]
    fn a_path_windows_names_otherwise_has_no_name_and_none_names_the_drives_folder() {
        assert_eq!(in_program(Path::new(r"\\server\share\file")), None);
        assert_eq!(in_program(Path::new(r"relative\file")), None);
        assert_eq!(on_this_computer(Path::new("/")), None);
        assert_eq!(on_this_computer(Path::new("/data/file")), None);
        assert_eq!(on_this_computer(Path::new("c/relative")), None);
    }

    #[test]
    fn a_resolved_drive_s_path_is_plain_and_a_share_s_is_kept() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\C:\Users\me\notes.txt")),
            PathBuf::from(r"C:\Users\me\notes.txt")
        );
        assert_eq!(plain(PathBuf::from(r"\\?\D:\")), PathBuf::from(r"D:\"));
        let share = PathBuf::from(r"\\?\UNC\server\share\file");
        assert_eq!(plain(share.clone()), share);
        assert_eq!(
            plain(PathBuf::from(r"C:\Users")),
            PathBuf::from(r"C:\Users")
        );
    }

    #[test]
    fn the_drives_are_listed_by_their_letter() {
        assert!(drives().contains(&"c".to_owned()), "{:?}", drives());
    }
}
