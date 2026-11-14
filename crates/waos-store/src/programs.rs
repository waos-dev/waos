//! Programs on disk. Each has a folder under `programs/`, named by its id, made by its install or
//! its first run, whichever comes first, and holding what the store keeps of it: when installed,
//! the folder its package unpacked into and `current`, a file naming that folder; its settings;
//! and its own folders, `data` and `cache`. A program run only in development has the last two.
//!
//! The invariant a later editor will break: replacing `current` is the one step that changes
//! what is installed, and moving the program's folder out of `programs/` the one step that
//! removes it; the system does each whole. An install unpacks into a folder of its own, under a
//! fresh name, before `current` names it, and the folder of the version it replaces is removed
//! only after; a removal moves the program's folder into the store's `removing/` before removing
//! it there. So a crash at any point leaves the earlier install or the new one, the program or
//! none of it, and [`clear_unfinished`] and the store's opening remove what else it left.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use waos_capabilities::ProgramFolders;
use waos_package::{MANIFEST, Manifest, PUBLISHER_KEY, Package, ProgramId, PublisherKey};

use crate::files::write_whole;
use crate::grants::SETTINGS;
use crate::{InstallError, InstalledProgram, StoreError};

/// The file naming the installed folder, in a program's folder.
const CURRENT: &str = "current";

/// The program's own folders, in its folder: what it keeps, and what it keeps that may be thrown
/// away.
const DATA: &str = "data";
const CACHE: &str = "cache";

/// What a program's folder keeps whether it is installed or not.
const KEPT: [&str; 3] = [SETTINGS, DATA, CACHE];

/// The folder `current` names in `program`, a program's folder: the one its package unpacked
/// into; none when no version of it is installed.
pub(crate) fn installed_folder(program: &Path) -> Result<Option<PathBuf>, StoreError> {
    let current = program.join(CURRENT);
    let text = match fs::read_to_string(&current) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::io_at(&current)(error)),
    };
    let name = text.trim_end();
    if !is_one_name(name) {
        let reason = format!("{name:?} is not the name of a folder beside it");
        return Err(StoreError::unreadable_at(&current)(reason));
    }
    Ok(Some(program.join(name)))
}

/// The program installed in `program`: its manifest and publisher key, as its package unpacked
/// them; none when no version of it is installed.
pub(crate) fn read_installed(program: &Path) -> Result<Option<InstalledProgram>, StoreError> {
    let Some(folder) = installed_folder(program)? else {
        return Ok(None);
    };
    let manifest_path = folder.join(MANIFEST);
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(StoreError::io_at(&manifest_path))?;
    let manifest =
        Manifest::from_toml(&manifest_text).map_err(StoreError::unreadable_at(&manifest_path))?;
    let key_path = folder.join(PUBLISHER_KEY);
    let key = fs::read(&key_path).map_err(StoreError::io_at(&key_path))?;
    let publisher = PublisherKey::from_bytes(&key).map_err(StoreError::unreadable_at(&key_path))?;
    Ok(Some(InstalledProgram {
        manifest,
        publisher,
    }))
}

/// Installs `package` in `program`, the folder of its id: unpacks it into a folder of its own,
/// makes `current` name that folder, then removes the version it replaced.
pub(crate) fn put_in_place(program: &Path, package: &mut Package) -> Result<(), InstallError> {
    fs::create_dir_all(program).map_err(StoreError::io_at(program))?;
    let unpacked = tempfile::Builder::new()
        .prefix(&format!("{}-", package.manifest().version()))
        .tempdir_in(program)
        .map_err(StoreError::io_at(program))?;
    package.unpack(unpacked.path())?;
    let installed = unpacked.keep();
    let name = installed
        .file_name()
        .expect("a folder made in another has a name");
    let current = program.join(CURRENT);
    write_whole(&current, name.as_encoded_bytes()).map_err(StoreError::io_at(&current))?;
    clear_unfinished(program)?;
    Ok(())
}

/// The program's own folders in `program`, its folder, made where they are not yet.
pub(crate) fn own_folders(program: &Path) -> Result<ProgramFolders, StoreError> {
    let folders = ProgramFolders {
        data: program.join(DATA),
        cache: program.join(CACHE),
    };
    for folder in [&folders.data, &folders.cache] {
        fs::create_dir_all(folder).map_err(StoreError::io_at(folder))?;
    }
    Ok(folders)
}

/// Removes the program `id`, installed in `program`, its folder, with all the folder keeps:
/// moves the folder into `removing`, which removes the program, then removes it there.
pub(crate) fn take_out(program: &Path, id: &ProgramId, removing: &Path) -> Result<(), StoreError> {
    if installed_folder(program)?.is_none() {
        return Err(StoreError::NotInstalled(id.clone()));
    }
    fs::create_dir_all(removing).map_err(StoreError::io_at(removing))?;
    let moved = removing.join(id.as_str());
    remove_if_there(&moved)?;
    fs::rename(program, &moved).map_err(StoreError::io_at(program))?;
    remove(&moved)
}

/// Removes from `program`, a program's folder, all but what it keeps (`current`, the folder it
/// names, the program's settings and its own folders): the version an update replaced, and what
/// an install a crash interrupted left. A folder left keeping nothing goes too.
pub(crate) fn clear_unfinished(program: &Path) -> Result<(), StoreError> {
    let installed = installed_folder(program)?;
    let entries = fs::read_dir(program).map_err(StoreError::io_at(program))?;
    let mut kept = 0;
    for entry in entries {
        let path = entry.map_err(StoreError::io_at(program))?.path();
        if is_kept(&path, installed.as_deref()) {
            kept += 1;
        } else {
            remove(&path)?;
        }
    }
    if kept == 0 {
        fs::remove_dir(program).map_err(StoreError::io_at(program))?;
    }
    Ok(())
}

/// Whether `path`, in a program's folder, is kept: `current` and the folder it names, the
/// program's settings and its own folders.
fn is_kept(path: &Path, installed: Option<&Path>) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    Some(path) == installed || name.is_some_and(|name| name == CURRENT || KEPT.contains(&name))
}

/// Removes whatever is at `path`, if anything is.
pub(crate) fn remove_if_there(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => remove(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::io_at(path)(error)),
    }
}

/// Removes the file or folder at `path`.
fn remove(path: &Path) -> Result<(), StoreError> {
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(StoreError::io_at(path))
}

/// Whether `name` is the name of one file or folder, not a path to another place.
fn is_one_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use waos_package::{COMPONENT, SigningKey, Version};

    use super::*;

    /// The start of a PNG of `side` by `side`, all an icon's check reads.
    fn png_header(side: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&side.to_be_bytes());
        png.extend_from_slice(&side.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        png
    }

    /// A package of `version` of `dev.example.triangle`, signed with `key`, written in `folder`.
    fn package(folder: &Path, version: &str, key: &SigningKey) -> Package {
        let manifest = Manifest::new(
            "dev.example.triangle".parse().unwrap(),
            "Triangle",
            version.parse().unwrap(),
        )
        .unwrap();
        let component = folder.join(format!("{version}.wasm"));
        fs::write(&component, format!("\0asm {version}")).unwrap();
        let icon = folder.join("icon.png");
        fs::write(&icon, png_header(256)).unwrap();
        let path = folder.join(format!("{version}.wapk"));
        waos_package::write(&path, &manifest, &component, &icon, key).unwrap();
        Package::open(&path).unwrap()
    }

    fn installed_version(program: &Path) -> Option<Version> {
        read_installed(program)
            .unwrap()
            .map(|installed| installed.manifest.version().clone())
    }

    fn entries(folder: &Path) -> usize {
        fs::read_dir(folder).unwrap().count()
    }

    #[test]
    fn an_install_is_the_folder_current_names() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let program = scratch.path().join("dev.example.triangle");
        put_in_place(&program, &mut package(scratch.path(), "1.0.0", &key)).unwrap();
        let installed = read_installed(&program).unwrap().unwrap();
        assert_eq!(installed.manifest.version(), &Version::new(1, 0, 0));
        assert_eq!(installed.publisher, key.publisher());
        let folder = installed_folder(&program).unwrap().unwrap();
        assert_eq!(fs::read(folder.join(COMPONENT)).unwrap(), b"\0asm 1.0.0");
        assert_eq!(entries(&program), 2);
    }

    #[test]
    fn an_update_removes_the_version_it_replaced() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let program = scratch.path().join("dev.example.triangle");
        put_in_place(&program, &mut package(scratch.path(), "1.0.0", &key)).unwrap();
        let earlier = installed_folder(&program).unwrap().unwrap();
        put_in_place(&program, &mut package(scratch.path(), "1.1.0", &key)).unwrap();
        assert_eq!(installed_version(&program), Some(Version::new(1, 1, 0)));
        assert!(!earlier.exists());
        assert_eq!(entries(&program), 2);
    }

    #[test]
    fn an_install_a_crash_interrupted_before_current_leaves_the_earlier_one() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let program = scratch.path().join("dev.example.triangle");
        put_in_place(&program, &mut package(scratch.path(), "1.0.0", &key)).unwrap();
        let interrupted = program.join("1.1.0-interrupted");
        package(scratch.path(), "1.1.0", &key)
            .unpack(&interrupted)
            .unwrap();
        fs::write(program.join(".tmp-current"), "1.1.0-interrupted").unwrap();
        assert_eq!(installed_version(&program), Some(Version::new(1, 0, 0)));
        clear_unfinished(&program).unwrap();
        assert!(!interrupted.exists());
        assert_eq!(entries(&program), 2);
        assert_eq!(installed_version(&program), Some(Version::new(1, 0, 0)));
    }

    #[test]
    fn a_first_install_a_crash_interrupted_leaves_no_program() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        package(scratch.path(), "1.0.0", &SigningKey::generate())
            .unpack(&program.join("1.0.0-interrupted"))
            .unwrap();
        assert_eq!(installed_version(&program), None);
        clear_unfinished(&program).unwrap();
        assert!(!program.exists());
    }

    #[test]
    fn a_program_run_in_development_keeps_its_folders_and_settings_uninstalled() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        let folders = own_folders(&program).unwrap();
        fs::write(folders.data.join("notes.txt"), "kept").unwrap();
        fs::write(program.join(SETTINGS), "").unwrap();
        fs::create_dir(program.join("1.0.0-interrupted")).unwrap();
        clear_unfinished(&program).unwrap();
        assert_eq!(fs::read(folders.data.join("notes.txt")).unwrap(), b"kept");
        assert!(folders.cache.is_dir());
        assert_eq!(entries(&program), 3, "data, cache and settings");

        let key = SigningKey::generate();
        put_in_place(&program, &mut package(scratch.path(), "1.0.0", &key)).unwrap();
        assert_eq!(entries(&program), 5, "its install beside them");
        assert_eq!(fs::read(folders.data.join("notes.txt")).unwrap(), b"kept");
    }

    #[test]
    fn taking_a_program_out_removes_its_folder_with_all_it_keeps() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        let removing = scratch.path().join("removing");
        let id: ProgramId = "dev.example.triangle".parse().unwrap();
        let mut triangle = package(scratch.path(), "1.0.0", &SigningKey::generate());
        put_in_place(&program, &mut triangle).unwrap();
        own_folders(&program).unwrap();
        take_out(&program, &id, &removing).unwrap();
        assert!(!program.exists());
        assert_eq!(entries(&removing), 0);
        assert!(matches!(
            take_out(&program, &id, &removing).unwrap_err(),
            StoreError::NotInstalled(_)
        ));
    }

    #[test]
    fn a_program_run_only_in_development_is_not_taken_out() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        let id: ProgramId = "dev.example.triangle".parse().unwrap();
        own_folders(&program).unwrap();
        let taken = take_out(&program, &id, &scratch.path().join("removing"));
        assert!(matches!(taken.unwrap_err(), StoreError::NotInstalled(_)));
        assert!(program.join(DATA).is_dir());
    }

    #[test]
    fn a_current_that_names_another_place_is_unreadable() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        fs::create_dir_all(&program).unwrap();
        for name in ["../elsewhere", "/elsewhere", "", "a/b"] {
            fs::write(program.join(CURRENT), name).unwrap();
            let error = installed_folder(&program).unwrap_err();
            assert!(
                matches!(error, StoreError::Unreadable { .. }),
                "{name}: {error}"
            );
        }
    }
}
