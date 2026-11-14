//! Writing and signing a package from a manifest, a component file and an icon file.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::entries::{
    COMPONENT, ICON, ICON_LIMIT, MANIFEST, MANIFEST_LIMIT, PUBLISHER_KEY, SIGNATURE,
};
use crate::icon::check_icon;
use crate::signed_message::{SignedDigests, sha256};
use crate::transfer::{check_within_limit, read_within_limit, stream};
use crate::{Manifest, PackageError, SigningKey};

/// Writes the package at `path`, replacing what is there: `manifest`, the component in the file
/// `program`, streamed, and the icon in the file `icon`, then the publisher key of `key` and the
/// signature `key` makes over the three, every entry deflated. Refuses what
/// [`Package::open`](crate::Package::open) would: an icon that is not a square PNG of at least
/// 256 pixels, and an entry over its cap.
pub fn write(
    path: &Path,
    manifest: &Manifest,
    program: &Path,
    icon: &Path,
    key: &SigningKey,
) -> Result<(), PackageError> {
    let manifest_text = manifest_text(manifest)?;
    let icon_png = read_icon(icon)?;
    let component = File::open(program).map_err(PackageError::io_at(program))?;
    let mut archive = create_archive(path)?;
    add_entry(&mut archive, MANIFEST, manifest_text.as_bytes(), path)?;
    add_entry(&mut archive, ICON, &icon_png, path)?;
    let component_sha256 = add_streamed_entry(&mut archive, COMPONENT, component, program, path)?;
    let signed = SignedDigests {
        manifest: sha256(manifest_text.as_bytes()),
        component: component_sha256,
        icon: sha256(&icon_png),
    };
    add_signature(&mut archive, &signed, key, path)?;
    archive
        .finish()
        .map_err(PackageError::writing_archive_at(path))?;
    Ok(())
}

fn manifest_text(manifest: &Manifest) -> Result<String, PackageError> {
    let text = manifest.to_toml();
    check_within_limit(MANIFEST, text.as_bytes(), MANIFEST_LIMIT)?;
    Ok(text)
}

fn read_icon(path: &Path) -> Result<Vec<u8>, PackageError> {
    let file = File::open(path).map_err(PackageError::io_at(path))?;
    let png = read_within_limit(file, ICON, ICON_LIMIT, path)?;
    check_icon(&png)?;
    Ok(png)
}

fn create_archive(path: &Path) -> Result<ZipWriter<File>, PackageError> {
    let file = File::create(path).map_err(PackageError::io_at(path))?;
    Ok(ZipWriter::new(file))
}

/// Adds the publisher key of `key` and the signature it makes over `signed`.
fn add_signature(
    archive: &mut ZipWriter<File>,
    signed: &SignedDigests,
    key: &SigningKey,
    path: &Path,
) -> Result<(), PackageError> {
    let signature = key.sign(signed.message().as_bytes());
    add_entry(archive, PUBLISHER_KEY, &key.publisher().to_bytes(), path)?;
    add_entry(archive, SIGNATURE, &signature.to_bytes(), path)
}

fn add_entry(
    archive: &mut ZipWriter<File>,
    name: &str,
    bytes: &[u8],
    path: &Path,
) -> Result<(), PackageError> {
    start_entry(archive, name, path)?;
    archive.write_all(bytes).map_err(PackageError::io_at(path))
}

/// Adds the entry `name`, streamed from `source`, the file at `source_path`, and answers its
/// SHA-256.
fn add_streamed_entry(
    archive: &mut ZipWriter<File>,
    name: &str,
    source: File,
    source_path: &Path,
    path: &Path,
) -> Result<[u8; 32], PackageError> {
    start_entry(archive, name, path)?;
    stream(source, source_path, archive, path)
}

fn start_entry(archive: &mut ZipWriter<File>, name: &str, path: &Path) -> Result<(), PackageError> {
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    archive
        .start_file(name, deflated)
        .map_err(PackageError::writing_archive_at(path))
}

#[cfg(test)]
mod tests {
    use zip::ZipArchive;

    use super::*;
    use crate::fixtures::{
        TRIANGLE_MANIFEST, component_bytes, png_header, triangle_package, write_file,
    };

    fn triangle_manifest() -> Manifest {
        Manifest::from_toml(TRIANGLE_MANIFEST).unwrap()
    }

    #[test]
    fn every_entry_is_deflated_and_the_signature_comes_last() {
        let scratch = tempfile::tempdir().unwrap();
        let path = triangle_package(scratch.path(), &SigningKey::generate());
        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).unwrap();
            assert_eq!(
                entry.compression(),
                CompressionMethod::Deflated,
                "{}",
                entry.name()
            );
            names.push(entry.name().to_owned());
        }
        assert_eq!(names, [MANIFEST, ICON, COMPONENT, PUBLISHER_KEY, SIGNATURE]);
    }

    #[test]
    fn an_icon_that_breaks_the_rules_is_not_written() {
        let scratch = tempfile::tempdir().unwrap();
        let component = write_file(scratch.path(), "program.wasm", &component_bytes());
        let icon = write_file(scratch.path(), "icon.png", &png_header(1024, 768));
        let path = scratch.path().join("program.wapk");
        let key = SigningKey::generate();
        let error = write(&path, &triangle_manifest(), &component, &icon, &key).unwrap_err();
        assert!(
            matches!(
                error,
                PackageError::IconNotSquare {
                    width: 1024,
                    height: 768
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_missing_component_file_is_named() {
        let scratch = tempfile::tempdir().unwrap();
        let component = scratch.path().join("program.wasm");
        let icon = write_file(scratch.path(), "icon.png", &png_header(1024, 1024));
        let path = scratch.path().join("program.wapk");
        let key = SigningKey::generate();
        let error = write(&path, &triangle_manifest(), &component, &icon, &key).unwrap_err();
        assert!(
            matches!(&error, PackageError::Io { path, .. } if *path == component),
            "{error}"
        );
        assert!(!path.exists());
    }
}
