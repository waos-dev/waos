//! Verifying a package as WaOS opens it: its signature, its manifest and its icon, by
//! `waos_package::Package::open`; and, where a publisher is named, that the package is signed with
//! that publisher's key, as a catalog that pins each app's key checks every package it collects
//! (decision 3 of `docs/proposals/2026-11-22-official-source.md`).
//!
//! The invariant: a package passes only what WaOS's own opening passes, never a check of this
//! module's alone, and the publisher is compared by its whole key, never its fingerprint.

use std::path::Path;

use anyhow::{Context, Result, bail};
use waos_package::{Package, PublisherKey};

/// The package at `file`, opened as WaOS opens it, refused unless it is signed with `publisher`'s
/// key where one is named.
pub fn verify(file: &Path, publisher: Option<&PublisherKey>) -> Result<Package> {
    let package =
        Package::open(file).with_context(|| format!("{} does not verify", file.display()))?;
    if let Some(named) = publisher {
        check_publisher(file, package.publisher(), named)?;
    }
    Ok(package)
}

/// What a verified package is, a line each: its id, its version, its publisher's key in full and
/// that key's fingerprint.
pub fn description(package: &Package) -> String {
    let manifest = package.manifest();
    let publisher = package.publisher();
    format!(
        "id           {}\nversion      {}\npublisher    {}\nfingerprint  {}\n",
        manifest.id(),
        manifest.version(),
        publisher.to_hex(),
        publisher.fingerprint()
    )
}

/// Refuses the package at `file`, signed with `signed`, unless that is the `named` key.
fn check_publisher(file: &Path, signed: &PublisherKey, named: &PublisherKey) -> Result<()> {
    if signed != named {
        bail!(
            "{} is signed by publisher {} ({}), not by the publisher named, {} ({})",
            file.display(),
            signed.to_hex(),
            signed.fingerprint(),
            named.to_hex(),
            named.fingerprint()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use waos_package::{Manifest, SigningKey};

    use super::*;

    /// A package of a component that imports nothing, signed with `key`, in `folder`.
    fn package_signed_with(folder: &Path, key: &SigningKey) -> PathBuf {
        let component = folder.join("component.wasm");
        fs::write(&component, wat::parse_str("(component)").unwrap()).unwrap();
        let icon = folder.join("icon.png");
        fs::write(&icon, crate::new::icon::placeholder()).unwrap();
        let manifest = Manifest::new(
            "dev.example.triangle".parse().unwrap(),
            "Triangle",
            "1.2.0".parse().unwrap(),
        )
        .unwrap();
        let package = folder.join("dev.example.triangle-1.2.0.wapk");
        waos_package::write(&package, &manifest, &component, &icon, key).unwrap();
        package
    }

    #[test]
    fn a_package_verifies_and_says_what_it_is_with_its_whole_key() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let package = verify(&package_signed_with(scratch.path(), &key), None).unwrap();
        let publisher = key.publisher();
        assert_eq!(
            description(&package),
            format!(
                "id           dev.example.triangle\nversion      1.2.0\npublisher    {}\nfingerprint  {}\n",
                publisher.to_hex(),
                publisher.fingerprint()
            )
        );
    }

    #[test]
    fn a_package_verifies_with_the_key_it_is_signed_with_named() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let file = package_signed_with(scratch.path(), &key);
        assert!(verify(&file, Some(&key.publisher())).is_ok());
    }

    #[test]
    fn a_package_signed_by_another_publisher_than_the_one_named_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let file = package_signed_with(scratch.path(), &SigningKey::generate());
        let named = SigningKey::generate().publisher();
        let error = verify(&file, Some(&named)).unwrap_err().to_string();
        assert!(error.contains("not by the publisher named"), "{error}");
        assert!(error.contains(&named.to_hex()), "{error}");
    }

    #[test]
    fn a_file_that_is_no_package_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("not.wapk");
        fs::write(&file, "not a zip").unwrap();
        let error = format!("{:#}", verify(&file, None).unwrap_err());
        assert!(error.contains("does not verify"), "{error}");
    }
}
