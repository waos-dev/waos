//! Packaging a program: its release component, signed with the user's signing key, written to
//! `target/waos/<id>-<version>.wapk` and listed in `target/waos/index.json`, so `target/waos`
//! holds what a package source serves, the index with its packages beside it.
//!
//! The invariants a later editor will break: the index lists a package by its file name, beside
//! it, with the SHA-256 of the file as written; and listing a program at a version the index
//! lists already replaces that listing and keeps every other.

use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use waos_package::{Index, ListedProgram, Manifest, ProgramKind};

use crate::build::{self, Features, Profile};
use crate::metadata::WaosMetadata;
use crate::{component_imports, icon_rule, signing_key, workspace};

/// The index's name, in `target/waos`.
const INDEX: &str = "index.json";

/// A program packaged and listed.
pub struct PackagedProgram {
    /// The package, under `target/waos`.
    pub package: PathBuf,
    /// The index it is listed in, beside it.
    pub index: PathBuf,
}

/// Packages `package`, or the package of this directory, built with `features`, and lists it in
/// the index.
pub fn package(name: Option<&str>, features: &Features) -> Result<PackagedProgram> {
    let metadata = workspace::metadata()?;
    let package = workspace::chosen_package(&metadata, name)?;
    let waos = WaosMetadata::of(package)?;
    warn_of_icon_breaches(&waos.icon)?;
    let manifest = Manifest::new(waos.id, waos.name, package.version.clone())
        .with_context(|| format!("the manifest of {}", package.name))?
        .with_kind(waos.kind)
        .with_category(waos.category);
    let key = signing_key::signing_key()?;
    let built = build::build(&metadata, package, Profile::Release, features)?;
    warn_of_an_app_with_no_view(&manifest, &built.component)?;
    let folder = workspace::waos_folder(&metadata);
    let written = folder.join(format!("{}-{}.wapk", manifest.id(), manifest.version()));
    waos_package::write(&written, &manifest, &built.component, &waos.icon, &key)
        .with_context(|| format!("writing {}", written.display()))?;
    let index = folder.join(INDEX);
    list_in_index(&index, &manifest, &written)?;
    Ok(PackagedProgram {
        package: written,
        index,
    })
}

/// Says on stderr that an app whose `component` never imports `wasdk:platform/views` can never
/// show a window, likely a program that is a tool and does not say so; packaging goes on. A tool
/// that imports it may open a window now and then, and nothing is said.
fn warn_of_an_app_with_no_view(manifest: &Manifest, component: &Path) -> Result<()> {
    if manifest.kind() != ProgramKind::App {
        return Ok(());
    }
    let bytes = fs::read(component).with_context(|| format!("reading {}", component.display()))?;
    if !component_imports::imports_views(&bytes)? {
        eprintln!(
            "waos: warning: {} is an app, but its component never asks for a view, so it can show no window; a program with no UI says `kind = \"tool\"` in its [package.metadata.waos]",
            manifest.id()
        );
    }
    Ok(())
}

/// Says on stderr what the icon at `icon` does against the rule of icons; packaging goes on.
fn warn_of_icon_breaches(icon: &Path) -> Result<()> {
    let png = fs::read(icon).with_context(|| format!("reading {}", icon.display()))?;
    for breach in icon_rule::breaches(&png) {
        eprintln!(
            "waos: warning: {}: {breach}; {}",
            icon.display(),
            icon_rule::RULE
        );
    }
    Ok(())
}

/// Lists the package at `package`, whose manifest is `manifest`, in the index at `index`, which
/// is made when there is none.
fn list_in_index(index: &Path, manifest: &Manifest, package: &Path) -> Result<()> {
    let mut listing = read_index(index)?;
    listing.list(ListedProgram {
        id: manifest.id().clone(),
        name: manifest.name().to_owned(),
        version: manifest.version().clone(),
        package: file_name(package),
        sha256: sha256_of_file(package)?,
    });
    fs::write(index, listing.to_json()).with_context(|| format!("writing {}", index.display()))
}

/// The index at `path`; an empty one when there is none.
fn read_index(path: &Path) -> Result<Index> {
    match fs::read(path) {
        Ok(json) => Index::from_json(&json)
            .with_context(|| format!("{} is not an index this can add to", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Index::default()),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn file_name(path: &Path) -> String {
    let name = path.file_name().expect("a package is a file");
    name.to_string_lossy().into_owned()
}

/// The SHA-256 of the file at `path`, read a buffer at a time.
fn sha256_of_file(path: &Path) -> Result<[u8; 32]> {
    let read = || -> std::io::Result<[u8; 32]> {
        let mut file = File::open(path)?;
        let mut sha256 = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            match file.read(&mut buffer)? {
                0 => return Ok(sha256.finalize().into()),
                read => sha256.update(&buffer[..read]),
            }
        }
    };
    read().with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use waos_package::Version;

    use super::*;

    fn manifest(id: &str, version: &str) -> Manifest {
        Manifest::new(id.parse().unwrap(), "Triangle", version.parse().unwrap()).unwrap()
    }

    /// A file standing in for a package: listing reads only its name and bytes.
    fn package_file(folder: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = folder.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn listings(index: &Path) -> Vec<(String, Version, String, String)> {
        let index = Index::from_json(&fs::read(index).unwrap()).unwrap();
        index
            .programs
            .into_iter()
            .map(|program| {
                (
                    program.id.to_string(),
                    program.version,
                    program.package,
                    hex::encode(program.sha256),
                )
            })
            .collect()
    }

    #[test]
    fn the_first_package_makes_the_index() {
        let scratch = tempfile::tempdir().unwrap();
        let index = scratch.path().join(INDEX);
        let package = package_file(scratch.path(), "dev.example.triangle-1.0.0.wapk", b"one");
        list_in_index(&index, &manifest("dev.example.triangle", "1.0.0"), &package).unwrap();
        assert_eq!(
            listings(&index),
            [(
                "dev.example.triangle".to_owned(),
                Version::new(1, 0, 0),
                "dev.example.triangle-1.0.0.wapk".to_owned(),
                // `printf one | shasum -a 256`
                "7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed".to_owned()
            )]
        );
    }

    #[test]
    fn a_version_packaged_again_replaces_its_listing_and_keeps_every_other() {
        let scratch = tempfile::tempdir().unwrap();
        let index = scratch.path().join(INDEX);
        let folder = scratch.path();
        let listed = [
            ("dev.example.triangle", "1.0.0", "old"),
            ("dev.example.triangle", "1.1.0", "first"),
            ("dev.example.square", "1.1.0", "square"),
            ("dev.example.triangle", "1.1.0", "again"),
        ];
        for (id, version, bytes) in listed {
            let name = format!("{id}-{version}.wapk");
            let package = package_file(folder, &name, bytes.as_bytes());
            list_in_index(&index, &manifest(id, version), &package).unwrap();
        }
        let again = hex::encode(Sha256::digest(b"again"));
        let kept: Vec<(String, String)> = listings(&index)
            .into_iter()
            .map(|(id, version, _, sha256)| (format!("{id} {version}"), sha256))
            .collect();
        assert_eq!(
            kept,
            [
                (
                    "dev.example.triangle 1.0.0".to_owned(),
                    hex::encode(Sha256::digest(b"old"))
                ),
                ("dev.example.triangle 1.1.0".to_owned(), again),
                (
                    "dev.example.square 1.1.0".to_owned(),
                    hex::encode(Sha256::digest(b"square"))
                ),
            ]
        );
    }

    #[test]
    fn an_index_that_cannot_be_read_is_left_as_it_is() {
        let scratch = tempfile::tempdir().unwrap();
        let index = scratch.path().join(INDEX);
        fs::write(&index, "not json").unwrap();
        let package = package_file(scratch.path(), "dev.example.triangle-1.0.0.wapk", b"one");
        let error = list_in_index(&index, &manifest("dev.example.triangle", "1.0.0"), &package)
            .unwrap_err();
        assert!(error.to_string().contains("not an index"), "{error}");
        assert_eq!(fs::read_to_string(&index).unwrap(), "not json");
    }
}
