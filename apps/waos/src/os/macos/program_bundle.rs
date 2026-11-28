//! A program's app bundle, as Chrome writes an app shim: an `Info.plist` with the namespaced bundle id `dev.waos.program.<id>`,
//! the program's name and version, and the keys its window process reads
//! (`waos_window_messages::bundle`); the kinds of file the program opens, each a document type
//! matched by its extensions and ranked `Alternate`, so the Finder offers the app in Open With and
//! never takes a type's double-click from the app the user opens it with; its icon, made from the
//! package's PNG with the system's own `sips` and `iconutil`; a copy of WaOS's loader as its
//! executable; signed ad hoc with the hardened runtime and the one entitlement the loader needs to load WaOS's library,
//! `com.apple.security.cs.disable-library-validation`, measured necessary (without it a hardened
//! loader cannot load the library); and registered with Launch Services, each time it is written,
//! which is how the system learns the kinds of file it opens.
//! Deleting one resets what the system granted it first, since `tccutil` finds a bundle id
//! through Launch Services, then unregisters it and removes it whole.
//!
//! The invariant: a bundle is written whole in a folder beside its place and moved into it, so a
//! bundle that is there is complete and signed; signing is `/usr/bin/codesign`'s, Chrome's
//! `SecCodeSigner` being private.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use waos_package::{Category, FileAssociation, ProgramId};
use waos_window_messages::bundle::{DATA_FOLDER_KEY, PROGRAM_ID_KEY, WAOS_KEY, WINDOW_LIBRARY_KEY};

use super::code_signature;

/// The executable's name in every bundle.
const LOADER: &str = "waos-program-loader";

/// The icon's name in the bundle's resources, as `CFBundleIconFile` names it.
const ICON: &str = "program";

const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// What a bundle is written of.
pub(crate) struct BundleContents<'a> {
    pub(crate) program: &'a ProgramId,
    /// The program's name as its user knows it, which the Dock and the menu bar show.
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
    /// The package's icon, a square PNG's bytes; none for a development build that had none.
    pub(crate) icon: Option<&'a [u8]>,
    /// What the app is for, where its author said: the heading macOS files it under.
    pub(crate) category: Option<Category>,
    /// The kinds of file the program opens, which the Finder offers it for.
    pub(crate) file_associations: &'a [FileAssociation],
    /// WaOS's files the bundle refers to.
    pub(crate) waos: &'a WaosFiles,
}

/// WaOS's own files a bundle refers to, and its data folder, as this WaOS has them.
#[derive(Clone, Debug)]
pub(crate) struct WaosFiles {
    /// The loader every bundle carries a copy of.
    pub(crate) loader: PathBuf,
    /// The window library the loader loads.
    pub(crate) window_library: PathBuf,
    /// The WaOS a window process starts when none answers: its app bundle, or its executable.
    pub(crate) waos: PathBuf,
    /// WaOS's data folder, whose bootstrap name a window process looks up.
    pub(crate) data_folder: PathBuf,
}

/// The bundle id of the program `program`'s app: namespaced, so no program claims another app's.
pub(crate) fn bundle_id(program: &ProgramId) -> String {
    format!("dev.waos.program.{program}")
}

/// Writes the bundle of `contents` at `path`, in place of any there, signs it and registers it;
/// answers its code hash, which its process is checked against.
pub(crate) fn write(path: &Path, contents: &BundleContents) -> io::Result<Vec<u8>> {
    let staged = staged_beside(path)?;
    write_files(staged.path(), contents)?;
    sign(staged.path())?;
    replace(path, staged)?;
    register(path)?;
    code_signature::code_hash_of_bundle(path)
}

/// Writes the bundle's files at `bundle`, unsigned.
pub(crate) fn write_files(bundle: &Path, contents: &BundleContents) -> io::Result<()> {
    let macos = bundle.join("Contents/MacOS");
    let resources = bundle.join("Contents/Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    fs::write(bundle.join("Contents/Info.plist"), info_plist(contents))?;
    fs::copy(&contents.waos.loader, macos.join(LOADER))?;
    if let Some(icon) = contents.icon {
        write_icon(icon, &resources.join(format!("{ICON}.icns")))?;
    }
    Ok(())
}

/// Takes back what the system granted the app at `path`, unregisters it and removes it.
pub(crate) fn delete(path: &Path, program: &ProgramId) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    // A bundle id unknown to the system's privacy settings is no failure.
    let _ = run(Command::new("/usr/bin/tccutil").args(["reset", "All", &bundle_id(program)]));
    let _ = run(Command::new(LSREGISTER).arg("-u").arg(path));
    fs::remove_dir_all(path)
}

/// The bundle's `Info.plist`.
fn info_plist(contents: &BundleContents) -> String {
    let waos = contents.waos;
    let mut entries = vec![
        ("CFBundleIdentifier", bundle_id(contents.program)),
        ("CFBundleName", contents.name.to_owned()),
        ("CFBundleDisplayName", contents.name.to_owned()),
        ("CFBundleExecutable", LOADER.to_owned()),
        ("CFBundlePackageType", "APPL".to_owned()),
        ("CFBundleInfoDictionaryVersion", "6.0".to_owned()),
        ("CFBundleShortVersionString", contents.version.to_owned()),
        ("CFBundleVersion", contents.version.to_owned()),
        ("NSPrincipalClass", "NSApplication".to_owned()),
        (PROGRAM_ID_KEY, contents.program.to_string()),
        (DATA_FOLDER_KEY, path_string(&waos.data_folder)),
        (WINDOW_LIBRARY_KEY, path_string(&waos.window_library)),
        (WAOS_KEY, path_string(&waos.waos)),
    ];
    if contents.icon.is_some() {
        entries.push(("CFBundleIconFile", ICON.to_owned()));
    }
    if let Some(category) = contents.category {
        // Apple's identifier is the manifest's spelling under its prefix.
        entries.push((
            "LSApplicationCategoryType",
            format!("public.app-category.{category}"),
        ));
    }
    let strings: String = entries
        .iter()
        .map(|(key, value)| {
            format!(
                "\t<key>{}</key>\n\t<string>{}</string>\n",
                escaped(key),
                escaped(value)
            )
        })
        .collect();
    let document_types = document_types(contents.file_associations);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n{strings}{document_types}\
         \t<key>NSHighResolutionCapable</key>\n\t<true/>\n</dict>\n</plist>\n"
    )
}

/// `CFBundleDocumentTypes`, one entry for each kind of file the program opens, matched by its
/// extensions, as an app that does not define the type is matched (Waft's apps' rule for a type
/// with no identifier): the app views such a file, and ranks `Alternate`, offered in the Finder's
/// Open With and never the app a double-click opens it with unless the user makes it so. Nothing
/// for a program that opens none.
fn document_types(associations: &[FileAssociation]) -> String {
    if associations.is_empty() {
        return String::new();
    }
    let entries: String = associations
        .iter()
        .map(|association| {
            let extensions: String = association
                .extensions
                .iter()
                .map(|extension| format!("\t\t\t\t<string>{}</string>\n", escaped(extension)))
                .collect();
            format!(
                "\t\t<dict>\n\
                 \t\t\t<key>CFBundleTypeName</key>\n\t\t\t<string>{}</string>\n\
                 \t\t\t<key>CFBundleTypeRole</key>\n\t\t\t<string>Viewer</string>\n\
                 \t\t\t<key>LSHandlerRank</key>\n\t\t\t<string>Alternate</string>\n\
                 \t\t\t<key>CFBundleTypeExtensions</key>\n\t\t\t<array>\n{extensions}\t\t\t</array>\n\
                 \t\t</dict>\n",
                escaped(&association.name)
            )
        })
        .collect();
    format!("\t<key>CFBundleDocumentTypes</key>\n\t<array>\n{entries}\t</array>\n")
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `text` as XML's character data.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The icon `png` as the `.icns` at `icns`: each size the system draws an app's icon at, made by
/// `sips` from a copy of it, joined by `iconutil`.
fn write_icon(png: &[u8], icns: &Path) -> io::Result<()> {
    let mut original = tempfile::Builder::new().suffix(".png").tempfile()?;
    original.write_all(png)?;
    let png = original.path();
    let iconset = tempfile::Builder::new().suffix(".iconset").tempdir()?;
    for side in [16, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let file = iconset
                .path()
                .join(format!("icon_{side}x{side}{suffix}.png"));
            let pixels = (side * scale).to_string();
            run(Command::new("/usr/bin/sips")
                .args(["-z", &pixels, &pixels])
                .arg(png)
                .arg("--out")
                .arg(&file))?;
        }
    }
    run(Command::new("/usr/bin/iconutil")
        .args(["--convert", "icns", "--output"])
        .arg(icns)
        .arg(iconset.path()))
}

/// Signs the bundle at `bundle` ad hoc, with the hardened runtime and the leave to load WaOS's
/// library.
fn sign(bundle: &Path) -> io::Result<()> {
    let entitlements = tempfile::Builder::new()
        .suffix(".entitlements")
        .tempfile()?;
    fs::write(
        entitlements.path(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>com.apple.security.cs.disable-library-validation</key>\n\t<true/>\n\
         </dict>\n</plist>\n",
    )?;
    run(Command::new("/usr/bin/codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
        ])
        .arg(entitlements.path())
        .arg(bundle))
}

/// Registers the app at `bundle` with Launch Services, so it opens by its bundle id and the system
/// knows its name and icon.
fn register(bundle: &Path) -> io::Result<()> {
    run(Command::new(LSREGISTER).arg("-f").arg(bundle))
}

/// A folder beside `path` to write a bundle in, removed unless it is moved into place.
fn staged_beside(path: &Path) -> io::Result<tempfile::TempDir> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("a bundle's path names a folder"))?;
    fs::create_dir_all(folder)?;
    tempfile::Builder::new()
        .prefix(".waos-bundle-")
        .suffix(".app")
        .tempdir_in(folder)
}

/// Puts the bundle written in `staged` at `path`, in place of the one there.
fn replace(path: &Path, staged: tempfile::TempDir) -> io::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::rename(staged.keep(), path)
}

/// Runs `command`, failing with what it wrote when it fails.
fn run(command: &mut Command) -> io::Result<()> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{:?} failed: {}",
        command.get_program(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(test)]
mod tests;
