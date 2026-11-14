//! A file the system opens with WaOS, double-clicked or chosen with the Finder's Open With: a
//! `.wapk`, the one type WaOS's bundle declares, which is opened, and so verified, before the user
//! is asked whether to install it.
//!
//! The invariant: nothing a package says is shown before `waos-package` has verified its
//! signature.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use waos_package::{Manifest, Package, PublisherKey};

/// The extension of WaOS's packages.
const PACKAGE_EXTENSION: &str = "wapk";

/// A package opened with WaOS, verified, waiting for the user to say whether to install it.
pub(crate) struct OpenedPackage {
    file: PathBuf,
    package: Package,
    /// Its icon, a PNG, kept so a widget showing it shows the same bytes each build.
    icon: Arc<[u8]>,
}

impl OpenedPackage {
    /// The package at `file`, verified; a file that is not a `.wapk`, or a package that does not
    /// verify, is refused with why.
    pub(crate) fn open(file: &Path) -> Result<OpenedPackage> {
        let is_package = file
            .extension()
            .is_some_and(|extension| extension == PACKAGE_EXTENSION);
        if !is_package {
            bail!(
                "{} is not a program WaOS installs: its packages end in .{PACKAGE_EXTENSION}",
                file.display()
            );
        }
        let package = Package::open(file)
            .with_context(|| format!("{} cannot be installed", file.display()))?;
        Ok(OpenedPackage {
            file: file.to_path_buf(),
            icon: Arc::from(package.icon()),
            package,
        })
    }

    /// Where it was opened from.
    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// What it says it is, as its signature vouches.
    pub(crate) fn manifest(&self) -> &Manifest {
        self.package.manifest()
    }

    /// Its icon, a PNG, as its signature vouches.
    pub(crate) fn icon(&self) -> &Arc<[u8]> {
        &self.icon
    }

    /// The key it is signed with: its publisher's.
    pub(crate) fn publisher(&self) -> &PublisherKey {
        self.package.publisher()
    }

    /// The package, to install.
    pub(crate) fn into_package(self) -> Package {
        self.package
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;

    use waos_package::SigningKey;

    use super::*;

    /// The side of a test package's icon, the least a package's icon check takes.
    pub(crate) const ICON_SIDE: u32 = 256;

    /// The colour of every pixel of a test package's icon.
    pub(crate) const ICON_PIXEL: [u8; 4] = [0x3b, 0x82, 0xf6, 0xff];

    /// A PNG of [`ICON_SIDE`] by [`ICON_SIDE`] of [`ICON_PIXEL`], as a package's icon is.
    fn icon() -> Vec<u8> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, ICON_SIDE, ICON_SIDE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels = ICON_PIXEL.repeat((ICON_SIDE * ICON_SIDE) as usize);
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        png
    }

    /// A package of `id` at `version`, named `name`, made of `component`, signed with `key` and
    /// written in `folder` as `<id>-<version>.wapk`.
    pub(crate) fn write_package(
        folder: &Path,
        id: &str,
        name: &str,
        version: &str,
        component: &[u8],
        key: &SigningKey,
    ) -> PathBuf {
        let manifest = Manifest::new(id.parse().unwrap(), name, version.parse().unwrap()).unwrap();
        let component_file = folder.join(format!("{id}-{version}.wasm"));
        fs::write(&component_file, component).unwrap();
        let icon_file = folder.join("icon.png");
        fs::write(&icon_file, icon()).unwrap();
        let path = folder.join(format!("{id}-{version}.wapk"));
        waos_package::write(&path, &manifest, &component_file, &icon_file, key).unwrap();
        path
    }

    #[test]
    fn a_package_opened_with_waos_is_verified_and_says_what_it_is() {
        let folder = tempfile::tempdir().unwrap();
        let file = write_package(
            folder.path(),
            "dev.example.triangle",
            "Triangle",
            "1.2.0",
            b"\0asm",
            &SigningKey::generate(),
        );
        let opened = OpenedPackage::open(&file).unwrap();
        assert_eq!(opened.manifest().name(), "Triangle");
        assert_eq!(opened.manifest().version().to_string(), "1.2.0");
        assert_eq!(opened.file(), file);
    }

    #[test]
    fn a_file_that_is_not_a_package_is_refused_with_why() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("notes.txt");
        fs::write(&file, "hello").unwrap();
        let error = OpenedPackage::open(&file).err().unwrap();
        assert!(format!("{error:#}").contains(".wapk"), "{error:#}");
    }

    #[test]
    fn a_package_that_does_not_verify_is_refused_with_why() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("broken.wapk");
        fs::write(&file, "not a zip").unwrap();
        let error = OpenedPackage::open(&file).err().unwrap();
        assert!(
            format!("{error:#}").contains("cannot be installed"),
            "{error:#}"
        );
    }
}
