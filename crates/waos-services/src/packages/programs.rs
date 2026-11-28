//! Apps on disk. Each has a folder under `programs/`, named by its id, holding `current`, a file naming
//! the folder its package, or its development build, unpacked into; beside them, what other
//! owners keep there (the app's settings and its own folders, `waos_services::store`'s), which this keeps
//! and never reads. What an install came from is in the folder it unpacked into: a package's
//! `publisher.key`, or a development build's `development.sha256`, its component's SHA-256;
//! beside them its `code`, the folder the compiler writes its component's code into.
//!
//! The invariant a later editor will break: replacing `current` is the one step that changes
//! what is installed, and moving the app's folder out of `programs/` the one step that removes
//! it; the system does each whole. An install unpacks into a folder of its own, under a fresh name
//! (it is staged), before `current` names it (it is committed), and the folder of the version it
//! replaces is removed only after; a version's files never change once `current` names it, but
//! for its code folder, which the compiler alone writes; a removal moves the app's folder into
//! `removing/` before removing it there. So a crash at any point leaves the earlier install or the
//! new one, the app or none of it, and [`clear_unfinished`] removes what else it left.

use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use waos_package::{COMPONENT, ICON, MANIFEST, Manifest, PUBLISHER_KEY, ProgramId, PublisherKey};

use crate::files::write_whole;
use crate::packages::error::{FolderError, InstallError, RemoveError};
use crate::packages::installed::{DevelopmentBuild, InstalledProgram, InstalledVersion, Origin};

/// The file of a development install's component's SHA-256, in the folder it unpacked into, in
/// place of a package's `publisher.key`.
const DEVELOPMENT_SHA256: &str = "development.sha256";

/// The file naming the installed folder, in a program's folder.
const CURRENT: &str = "current";

/// The code folder, in the folder a version unpacked into: the code the engine compiles from its
/// component, which goes with the version.
pub(crate) const CODE: &str = "code";

/// What others keep in an app's folder, installed or not, which an install keeps and a removal
/// takes with the folder: `waos_services::store`'s settings of the app and the app's own folders.
const KEPT_BY_OTHERS: [&str; 3] = ["settings.toml", "data", "cache"];

/// The folder `current` names in `program`, a program's folder: the one its package unpacked
/// into; none when no version of it is installed.
pub(crate) fn installed_folder(program: &Path) -> Result<Option<PathBuf>, FolderError> {
    let current = program.join(CURRENT);
    let text = match fs::read_to_string(&current) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(FolderError::io_at(&current)(error)),
    };
    let name = text.trim_end();
    if !is_one_name(name) {
        let reason = format!("{name:?} is not the name of a folder beside it");
        return Err(FolderError::unreadable_at(&current)(reason));
    }
    Ok(Some(program.join(name)))
}

/// The program installed in `program`: its manifest and where it came from, as its install
/// unpacked them; none when no version of it is installed.
pub(crate) fn read_installed(program: &Path) -> Result<Option<InstalledProgram>, FolderError> {
    let Some(folder) = installed_folder(program)? else {
        return Ok(None);
    };
    let manifest_path = folder.join(MANIFEST);
    let manifest_text =
        fs::read_to_string(&manifest_path).map_err(FolderError::io_at(&manifest_path))?;
    let manifest =
        Manifest::from_toml(&manifest_text).map_err(FolderError::unreadable_at(&manifest_path))?;
    Ok(Some(InstalledProgram {
        manifest,
        origin: read_origin(&folder)?,
    }))
}

/// Where the install unpacked in `folder` came from: the package its `publisher.key` names the
/// publisher of, or, with none, the development build its `development.sha256` names the
/// component of.
fn read_origin(folder: &Path) -> Result<Origin, FolderError> {
    let key_path = folder.join(PUBLISHER_KEY);
    match fs::read(&key_path) {
        Ok(key) => {
            let publisher =
                PublisherKey::from_bytes(&key).map_err(FolderError::unreadable_at(&key_path))?;
            return Ok(Origin::Package { publisher });
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(FolderError::io_at(&key_path)(error)),
    }
    let sha256_path = folder.join(DEVELOPMENT_SHA256);
    let sha256 = fs::read(&sha256_path).map_err(FolderError::io_at(&sha256_path))?;
    let component_sha256 = sha256.try_into().map_err(|_| {
        FolderError::unreadable_at(&sha256_path)("it is not the 32 bytes of a SHA-256")
    })?;
    Ok(Origin::Development { component_sha256 })
}

/// A version of an app unpacked and checked, not installed yet ([`commit`]): nothing installed
/// changed, and its folder goes when this is dropped uncommitted.
pub(crate) struct Staged {
    program: InstalledProgram,
    folder: TempDir,
}

impl Staged {
    /// The app as it is installed once committed.
    pub(crate) fn program(&self) -> &InstalledProgram {
        &self.program
    }

    /// The version unpacked, whose component is compiled before it is committed.
    pub(crate) fn version(&self) -> InstalledVersion {
        InstalledVersion {
            folder: self.folder.path().to_owned(),
        }
    }
}

/// Unpacks `program` into a folder of its own in `folder`, the folder of its id, as `unpack`
/// writes it: nothing installed changes, and the folder goes when the answer is dropped
/// uncommitted.
pub(crate) fn stage(
    folder: &Path,
    program: InstalledProgram,
    unpack: impl FnOnce(&Path) -> Result<(), InstallError>,
) -> Result<Staged, InstallError> {
    fs::create_dir_all(folder).map_err(FolderError::io_at(folder))?;
    let unpacked = tempfile::Builder::new()
        .prefix(&format!("{}-", program.manifest.version()))
        .tempdir_in(folder)
        .map_err(FolderError::io_at(folder))?;
    unpack(unpacked.path())?;
    Ok(Staged {
        program,
        folder: unpacked,
    })
}

/// Unpacks `build` as its app's development install in `folder`, the folder of its development
/// id (`dev.example.notes.development`), an app of its own beside one installed under the plain
/// id: new, or in place of the development build installed before, whatever its version. No
/// package holds a development id, which `waos-package` refuses. Nothing installed changes.
pub(crate) fn stage_development(
    folder: &Path,
    build: &DevelopmentBuild,
) -> Result<Staged, InstallError> {
    let manifest = DevelopmentBuild::installed_manifest(&build.manifest);
    let program = InstalledProgram {
        manifest: manifest.clone(),
        origin: Origin::Development {
            component_sha256: Sha256::digest(&build.component).into(),
        },
    };
    stage(folder, program, |unpacked| {
        Ok(unpack_development(
            unpacked,
            &manifest,
            &build.component,
            build.icon.as_deref(),
        )?)
    })
}

/// Installs `staged`, a version [`stage`] unpacked in `program`: `current` names it, then the
/// version it replaced is removed. One that cannot be, as a file another process maps on
/// Windows, stays for [`clear_unfinished`] at the package manager's next opening.
pub(crate) fn commit(program: &Path, staged: Staged) -> Result<InstalledProgram, InstallError> {
    if !staged.folder.path().is_dir() {
        let reason = "the app was removed while it was being installed";
        return Err(FolderError::unreadable_at(staged.folder.path())(reason).into());
    }
    let replaced = installed_folder(program)?;
    let installed = staged.folder.keep();
    let name = installed
        .file_name()
        .expect("a folder made in another has a name");
    let current = program.join(CURRENT);
    write_whole(&current, name.as_encoded_bytes()).map_err(FolderError::io_at(&current))?;
    if let Some(replaced) = replaced
        && let Err(error) = remove(&replaced)
    {
        tracing::warn!("the version replaced stays until the package manager opens again: {error}");
    }
    Ok(staged.program)
}

/// Writes a development build into `folder`, as a package unpacks: `manifest`, `component`, the
/// icon where it has one, and the component's SHA-256, each on disk before the install goes on.
pub(crate) fn unpack_development(
    folder: &Path,
    manifest: &Manifest,
    component: &[u8],
    icon: Option<&[u8]>,
) -> Result<(), FolderError> {
    write_synced(&folder.join(MANIFEST), manifest.to_toml().as_bytes())?;
    if let Some(icon) = icon {
        write_synced(&folder.join(ICON), icon)?;
    }
    write_synced(&folder.join(COMPONENT), component)?;
    let sha256: [u8; 32] = Sha256::digest(component).into();
    write_synced(&folder.join(DEVELOPMENT_SHA256), &sha256)
}

/// The icon of the install unpacked in `folder`, its PNG's bytes; none for a development build
/// that had none.
pub(crate) fn read_icon(folder: &Path) -> Result<Option<Vec<u8>>, FolderError> {
    let path = folder.join(ICON);
    match fs::read(&path) {
        Ok(icon) => Ok(Some(icon)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(FolderError::io_at(&path)(error)),
    }
}

/// Writes `contents` as the new file at `path`, on disk before it returns.
fn write_synced(path: &Path, contents: &[u8]) -> Result<(), FolderError> {
    let written = File::create(path).and_then(|mut file| {
        file.write_all(contents)?;
        file.sync_all()
    });
    written.map_err(FolderError::io_at(path))
}

/// Removes the program `id`, installed in `program`, its folder, with all the folder keeps:
/// moves the folder into `removing`, which removes the program, then removes it there.
pub(crate) fn take_out(program: &Path, id: &ProgramId, removing: &Path) -> Result<(), RemoveError> {
    if installed_folder(program)?.is_none() {
        return Err(RemoveError::NotInstalled(id.clone()));
    }
    fs::create_dir_all(removing).map_err(FolderError::io_at(removing))?;
    let moved = removing.join(id.as_str());
    remove_if_there(&moved)?;
    fs::rename(program, &moved).map_err(FolderError::io_at(program))?;
    Ok(remove(&moved)?)
}

/// Removes from `program`, a program's folder, all but what it keeps (`current`, the folder it
/// names, the program's settings and its own folders): the version an update replaced, and what
/// an install a crash interrupted left. A folder left keeping nothing goes too.
pub(crate) fn clear_unfinished(program: &Path) -> Result<(), FolderError> {
    let installed = installed_folder(program)?;
    let entries = fs::read_dir(program).map_err(FolderError::io_at(program))?;
    let mut kept = 0;
    for entry in entries {
        let path = entry.map_err(FolderError::io_at(program))?.path();
        if is_kept(&path, installed.as_deref()) {
            kept += 1;
        } else {
            remove(&path)?;
        }
    }
    if kept == 0 {
        fs::remove_dir(program).map_err(FolderError::io_at(program))?;
    }
    Ok(())
}

/// Whether `path`, in a program's folder, is kept: `current` and the folder it names, the
/// program's settings and its own folders.
fn is_kept(path: &Path, installed: Option<&Path>) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    Some(path) == installed
        || name.is_some_and(|name| name == CURRENT || KEPT_BY_OTHERS.contains(&name))
}

/// Removes whatever is at `path`, if anything is.
pub(crate) fn remove_if_there(path: &Path) -> Result<(), FolderError> {
    match fs::symlink_metadata(path) {
        Ok(_) => remove(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FolderError::io_at(path)(error)),
    }
}

/// Removes the file or folder at `path`.
fn remove(path: &Path) -> Result<(), FolderError> {
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(FolderError::io_at(path))
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

    use waos_package::{Package, SigningKey, Version};

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

    /// Installs `package` in `program`, as the package manager installs one.
    fn put_package(program: &Path, mut package: Package) {
        let installed = InstalledProgram {
            manifest: package.manifest().clone(),
            origin: Origin::Package {
                publisher: *package.publisher(),
            },
        };
        let staged = stage(program, installed, |folder| Ok(package.unpack(folder)?)).unwrap();
        commit(program, staged).unwrap();
    }

    fn installed_version(program: &Path) -> Option<Version> {
        read_installed(program)
            .unwrap()
            .map(|installed| installed.manifest.version().clone())
    }

    /// The app's own folders in `program`, as app storage makes them at its first run.
    fn others_folders(program: &Path) -> (PathBuf, PathBuf) {
        let (data, cache) = (program.join("data"), program.join("cache"));
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&cache).unwrap();
        (data, cache)
    }

    fn entries(folder: &Path) -> usize {
        fs::read_dir(folder).unwrap().count()
    }

    #[test]
    fn an_install_is_the_folder_current_names() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let program = scratch.path().join("dev.example.triangle");
        put_package(&program, package(scratch.path(), "1.0.0", &key));
        let installed = read_installed(&program).unwrap().unwrap();
        assert_eq!(installed.manifest.version(), &Version::new(1, 0, 0));
        assert_eq!(installed.publisher(), Some(key.publisher()));
        let folder = installed_folder(&program).unwrap().unwrap();
        assert_eq!(fs::read(folder.join(COMPONENT)).unwrap(), b"\0asm 1.0.0");
        assert_eq!(entries(&program), 2);
    }

    #[test]
    fn an_update_removes_the_version_it_replaced() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let program = scratch.path().join("dev.example.triangle");
        put_package(&program, package(scratch.path(), "1.0.0", &key));
        let earlier = installed_folder(&program).unwrap().unwrap();
        put_package(&program, package(scratch.path(), "1.1.0", &key));
        assert_eq!(installed_version(&program), Some(Version::new(1, 1, 0)));
        assert!(!earlier.exists());
        assert_eq!(entries(&program), 2);
    }

    #[test]
    fn an_install_a_crash_interrupted_before_current_leaves_the_earlier_one() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let program = scratch.path().join("dev.example.triangle");
        put_package(&program, package(scratch.path(), "1.0.0", &key));
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
        let (data, cache) = others_folders(&program);
        fs::write(data.join("notes.txt"), "kept").unwrap();
        fs::write(program.join("settings.toml"), "").unwrap();
        fs::create_dir(program.join("1.0.0-interrupted")).unwrap();
        clear_unfinished(&program).unwrap();
        assert_eq!(fs::read(data.join("notes.txt")).unwrap(), b"kept");
        assert!(cache.is_dir());
        assert_eq!(entries(&program), 3, "data, cache and settings");

        let key = SigningKey::generate();
        put_package(&program, package(scratch.path(), "1.0.0", &key));
        assert_eq!(entries(&program), 5, "its install beside them");
        assert_eq!(fs::read(data.join("notes.txt")).unwrap(), b"kept");
    }

    #[test]
    fn taking_a_program_out_removes_its_folder_with_all_it_keeps() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        let removing = scratch.path().join("removing");
        let id: ProgramId = "dev.example.triangle".parse().unwrap();
        put_package(
            &program,
            package(scratch.path(), "1.0.0", &SigningKey::generate()),
        );
        others_folders(&program);
        take_out(&program, &id, &removing).unwrap();
        assert!(!program.exists());
        assert_eq!(entries(&removing), 0);
        assert!(matches!(
            take_out(&program, &id, &removing).unwrap_err(),
            RemoveError::NotInstalled(_)
        ));
    }

    #[test]
    fn a_program_run_only_in_development_is_not_taken_out() {
        let scratch = tempfile::tempdir().unwrap();
        let program = scratch.path().join("dev.example.triangle");
        let id: ProgramId = "dev.example.triangle".parse().unwrap();
        let (data, _) = others_folders(&program);
        let taken = take_out(&program, &id, &scratch.path().join("removing"));
        assert!(matches!(taken.unwrap_err(), RemoveError::NotInstalled(_)));
        assert!(data.is_dir());
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
                matches!(error, FolderError::Unreadable { .. }),
                "{name}: {error}"
            );
        }
    }
}
