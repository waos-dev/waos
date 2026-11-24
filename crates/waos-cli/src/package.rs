//! Packaging a program: its release component, signed with the user's signing key, written to
//! `target/waos/<id>-<version>.wapk` and listed in `target/waos/index.json` with what a store
//! shows of it, its icon and screenshots copied beside the index as `<id>-<version>.png` and
//! `<id>-<version>-<n>.png`, so `target/waos` holds what a package source serves: the index with
//! its packages and their pictures beside it.
//!
//! The invariants a later editor will break: the index lists a package, and each picture, by its
//! file name, beside it, the package with the SHA-256 of the file as written; listing a program
//! at a version the index lists already replaces that listing and keeps every other, and what
//! the source says of itself, its name and what it puts forward.

use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
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
    let shown = Shown {
        icon: Some(waos.icon),
        summary: waos.summary.or_else(|| package.description.clone()),
        description: waos.description,
        publisher: waos.publisher,
        screenshots: waos.screenshots,
        released: Some(today()),
    };
    list_in_index(&index, &manifest, &written, &shown)?;
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

/// What a store shows of a program before it is downloaded, as its `[package.metadata.waos]`
/// gives it, each where it does.
#[derive(Debug, Default)]
struct Shown {
    /// Its icon's PNG.
    icon: Option<PathBuf>,
    summary: Option<String>,
    /// The text file of its description.
    description: Option<PathBuf>,
    publisher: Option<String>,
    /// Its screenshots, PNGs.
    screenshots: Vec<PathBuf>,
    released: Option<Date>,
}

/// Lists the package at `package`, whose manifest is `manifest`, in the index at `index`, which
/// is made when there is none, with what a store shows of it, `shown`, its pictures copied beside
/// the index.
fn list_in_index(index: &Path, manifest: &Manifest, package: &Path, shown: &Shown) -> Result<()> {
    let mut listing = read_index(index)?;
    let folder = index.parent().expect("an index is in a folder");
    let pictures = format!("{}-{}", manifest.id(), manifest.version());
    let icon = shown
        .icon
        .as_deref()
        .map(|icon| picture_beside_the_index(icon, folder, &format!("{pictures}.png")))
        .transpose()?;
    let screenshots = shown
        .screenshots
        .iter()
        .enumerate()
        .map(|(nth, screenshot)| {
            let name = format!("{pictures}-{}.png", nth + 1);
            picture_beside_the_index(screenshot, folder, &name)
        })
        .collect::<Result<Vec<String>>>()?;
    let description = shown
        .description
        .as_deref()
        .map(|text| fs::read_to_string(text).with_context(|| format!("reading {}", text.display())))
        .transpose()?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    listing.list(ListedProgram {
        icon,
        summary: shown.summary.clone(),
        description,
        category: manifest.category(),
        publisher: shown.publisher.clone(),
        released: shown.released,
        screenshots,
        ..ListedProgram::new(
            manifest.id().clone(),
            manifest.name(),
            manifest.version().clone(),
            file_name(package),
            sha256_of_file(package)?,
        )
    });
    fs::write(index, listing.to_json()).with_context(|| format!("writing {}", index.display()))
}

/// Copies the PNG at `picture` into `folder` as `name`, and answers `name`, as the index lists
/// it. A picture that is no PNG by its name is refused: a store shows PNGs, as an icon is.
fn picture_beside_the_index(picture: &Path, folder: &Path, name: &str) -> Result<String> {
    let is_png = picture
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    if !is_png {
        bail!(
            "{} is not a PNG, which a store's pictures are",
            picture.display()
        );
    }
    fs::copy(picture, folder.join(name))
        .with_context(|| format!("copying {} beside the index", picture.display()))?;
    Ok(name.to_owned())
}

/// Today, as a release is dated: the day in UTC.
fn today() -> Date {
    Timestamp::now().to_zoned(TimeZone::UTC).date()
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
        list_in_index(
            &index,
            &manifest("dev.example.triangle", "1.0.0"),
            &package,
            &Shown::default(),
        )
        .unwrap();
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
            list_in_index(&index, &manifest(id, version), &package, &Shown::default()).unwrap();
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
    fn a_listing_says_what_a_store_shows_with_its_pictures_beside_the_index() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path();
        let index = folder.join(INDEX);
        let source = folder.join("source");
        fs::create_dir(&source).unwrap();
        for (file, bytes) in [
            ("icon.png", "icon"),
            ("one.png", "first"),
            ("about.txt", "\nA triangle.\n\nNothing else.\n"),
        ] {
            fs::write(source.join(file), bytes).unwrap();
        }
        let package = package_file(folder, "dev.example.triangle-1.0.0.wapk", b"one");
        let shown = Shown {
            icon: Some(source.join("icon.png")),
            summary: Some("One triangle".to_owned()),
            description: Some(source.join("about.txt")),
            publisher: Some("Example".to_owned()),
            screenshots: vec![source.join("one.png")],
            released: Some("2026-11-23".parse().unwrap()),
        };
        let manifest = manifest("dev.example.triangle", "1.0.0")
            .with_category(Some(waos_package::Category::GraphicsDesign));
        list_in_index(&index, &manifest, &package, &shown).unwrap();

        let listed = Index::from_json(&fs::read(&index).unwrap())
            .unwrap()
            .programs
            .remove(0);
        assert_eq!(
            listed.icon.as_deref(),
            Some("dev.example.triangle-1.0.0.png")
        );
        assert_eq!(listed.screenshots, ["dev.example.triangle-1.0.0-1.png"]);
        assert_eq!(
            fs::read(folder.join("dev.example.triangle-1.0.0.png")).unwrap(),
            b"icon"
        );
        assert_eq!(
            fs::read(folder.join("dev.example.triangle-1.0.0-1.png")).unwrap(),
            b"first"
        );
        assert_eq!(
            listed.description.as_deref(),
            Some("A triangle.\n\nNothing else.")
        );
        assert_eq!(listed.summary.as_deref(), Some("One triangle"));
        assert_eq!(
            listed.category,
            Some(waos_package::Category::GraphicsDesign)
        );
        assert_eq!(listed.publisher.as_deref(), Some("Example"));
        assert_eq!(listed.released, Some("2026-11-23".parse().unwrap()));
    }

    #[test]
    fn a_screenshot_that_is_no_png_is_refused_and_what_the_source_says_of_itself_is_kept() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path();
        let index = folder.join(INDEX);
        fs::write(
            &index,
            r#"{"name": "Example Apps", "featured": ["dev.example.triangle"], "programs": []}"#,
        )
        .unwrap();
        let package = package_file(folder, "dev.example.triangle-1.0.0.wapk", b"one");
        fs::write(folder.join("shot.jpg"), "jpeg").unwrap();
        let with_a_jpeg = Shown {
            screenshots: vec![folder.join("shot.jpg")],
            ..Shown::default()
        };
        let manifest = manifest("dev.example.triangle", "1.0.0");
        let refusal = list_in_index(&index, &manifest, &package, &with_a_jpeg).unwrap_err();
        assert!(refusal.to_string().contains("is not a PNG"), "{refusal}");

        list_in_index(&index, &manifest, &package, &Shown::default()).unwrap();
        let listing = Index::from_json(&fs::read(&index).unwrap()).unwrap();
        assert_eq!(listing.name.as_deref(), Some("Example Apps"));
        assert_eq!(listing.featured.len(), 1);
        assert_eq!(listing.programs.len(), 1);
    }

    #[test]
    fn an_index_that_cannot_be_read_is_left_as_it_is() {
        let scratch = tempfile::tempdir().unwrap();
        let index = scratch.path().join(INDEX);
        fs::write(&index, "not json").unwrap();
        let package = package_file(scratch.path(), "dev.example.triangle-1.0.0.wapk", b"one");
        let error = list_in_index(
            &index,
            &manifest("dev.example.triangle", "1.0.0"),
            &package,
            &Shown::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not an index"), "{error}");
        assert_eq!(fs::read_to_string(&index).unwrap(), "not json");
    }
}
