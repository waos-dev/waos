//! Opening a package and unpacking it. Opening verifies the signature before anything the
//! package says is believed: the manifest and icon are read whole within their caps, the
//! component is inflated once for its SHA-256 and left compressed, and the publisher's key checks
//! the signature over the three; only then are the manifest and icon checked as what they say
//! they are. Unpacking inflates the component again, straight to disk, and refuses it unless it is
//! the one opening verified.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use ed25519_dalek::Signature;
use zip::ZipArchive;

use crate::entries::{
    COMPONENT, ICON, ICON_LIMIT, MANIFEST, MANIFEST_LIMIT, PUBLISHER_KEY, PUBLISHER_KEY_LENGTH,
    SIGNATURE, SIGNATURE_LENGTH,
};
use crate::icon::check_icon;
use crate::signed_message::{SignedDigests, sha256};
use crate::transfer::{read_within_limit, sha256_of, stream};
use crate::{Manifest, PackageError, PublisherKey};

/// An open package whose signature verifies with its publisher's key, and whose manifest and
/// icon are valid.
#[derive(Debug)]
pub struct Package {
    path: PathBuf,
    archive: ZipArchive<File>,
    manifest: Manifest,
    manifest_text: String,
    icon: Vec<u8>,
    publisher: PublisherKey,
    /// The component's SHA-256, as the signature covers it.
    component_sha256: [u8; 32],
}

impl Package {
    /// Opens the package at `path`: verifies its signature with its publisher's key, then reads
    /// and checks its manifest, which no development install's id ending in `.development` passes,
    /// and its icon. The component is read through once, for its SHA-256,
    /// and left compressed.
    pub fn open(path: &Path) -> Result<Package, PackageError> {
        let mut archive = open_archive(path)?;
        let signed = SignedEntries::read(&mut archive, path)?;
        let publisher = verify_signature(&mut archive, path, &signed)?;
        let manifest_text =
            String::from_utf8(signed.manifest).map_err(|_| PackageError::ManifestNotUtf8)?;
        let manifest = Manifest::from_toml(&manifest_text)?;
        manifest.check_published()?;
        check_icon(&signed.icon)?;
        Ok(Package {
            path: path.to_path_buf(),
            archive,
            manifest,
            manifest_text,
            icon: signed.icon,
            publisher,
            component_sha256: signed.component_sha256,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The program's icon, a square PNG, as the signature vouches.
    pub fn icon(&self) -> &[u8] {
        &self.icon
    }

    /// The key the package is signed with, which an installer holds every update to.
    pub fn publisher(&self) -> &PublisherKey {
        &self.publisher
    }

    /// Writes the program and its publisher's key into `directory`, creating it: [`MANIFEST`],
    /// [`ICON`] and [`PUBLISHER_KEY`] as they were read, and [`COMPONENT`] streamed from the
    /// archive and refused unless it is the component opening verified. The signature is not
    /// written: it did its work when the package was opened.
    ///
    /// Each file is on disk, not only in the system's cache, when this returns, so an installer
    /// can put the directory in place knowing it is whole. What was written before an error
    /// stays, so an installer unpacks into a directory of its own.
    pub fn unpack(&mut self, directory: &Path) -> Result<(), PackageError> {
        fs::create_dir_all(directory).map_err(PackageError::io_at(directory))?;
        write_synced(&directory.join(MANIFEST), self.manifest_text.as_bytes())?;
        write_synced(&directory.join(ICON), &self.icon)?;
        write_synced(&directory.join(PUBLISHER_KEY), &self.publisher.to_bytes())?;
        self.unpack_component(&directory.join(COMPONENT))
    }

    fn unpack_component(&mut self, destination: &Path) -> Result<(), PackageError> {
        let index = entry_index(&self.archive, COMPONENT)?;
        let component = self
            .archive
            .by_index(index)
            .map_err(PackageError::reading_archive_at(&self.path))?;
        let mut file = File::create(destination).map_err(PackageError::io_at(destination))?;
        let sha256 = stream(component, &self.path, &mut file, destination)?;
        if sha256 != self.component_sha256 {
            return Err(PackageError::ChangedSinceOpened);
        }
        file.sync_all().map_err(PackageError::io_at(destination))
    }
}

/// The entries the signature covers, as read from a package and not yet believed: the manifest
/// and icon whole, the component as its SHA-256.
struct SignedEntries {
    manifest: Vec<u8>,
    icon: Vec<u8>,
    component_sha256: [u8; 32],
}

impl SignedEntries {
    fn read(archive: &mut ZipArchive<File>, path: &Path) -> Result<SignedEntries, PackageError> {
        Ok(SignedEntries {
            manifest: read_entry(archive, MANIFEST, MANIFEST_LIMIT, path)?,
            icon: read_entry(archive, ICON, ICON_LIMIT, path)?,
            component_sha256: component_sha256(archive, path)?,
        })
    }

    fn digests(&self) -> SignedDigests {
        SignedDigests {
            manifest: sha256(&self.manifest),
            component: self.component_sha256,
            icon: sha256(&self.icon),
        }
    }
}

/// Reads the package's publisher key and signature, and answers the key once the signature
/// verifies with it over `signed`.
fn verify_signature(
    archive: &mut ZipArchive<File>,
    path: &Path,
    signed: &SignedEntries,
) -> Result<PublisherKey, PackageError> {
    let publisher = read_publisher_key(archive, path)?;
    let signature = read_signature(archive, path)?;
    publisher.verify(signed.digests().message().as_bytes(), &signature)?;
    Ok(publisher)
}

fn read_publisher_key(
    archive: &mut ZipArchive<File>,
    path: &Path,
) -> Result<PublisherKey, PackageError> {
    let bytes = read_entry(archive, PUBLISHER_KEY, PUBLISHER_KEY_LENGTH as u64, path)?;
    PublisherKey::from_bytes(&bytes).map_err(PackageError::PublisherKey)
}

fn read_signature(archive: &mut ZipArchive<File>, path: &Path) -> Result<Signature, PackageError> {
    let bytes = read_entry(archive, SIGNATURE, SIGNATURE_LENGTH as u64, path)?;
    let length = bytes.len();
    let bytes: [u8; SIGNATURE_LENGTH] = bytes
        .try_into()
        .map_err(|_| PackageError::SignatureNotEd25519 { length })?;
    Ok(Signature::from_bytes(&bytes))
}

fn open_archive(path: &Path) -> Result<ZipArchive<File>, PackageError> {
    let file = File::open(path).map_err(PackageError::io_at(path))?;
    ZipArchive::new(file).map_err(PackageError::reading_archive_at(path))
}

/// Reads the whole of the entry `name` of `archive`, the package at `path`, within `limit`.
fn read_entry(
    archive: &mut ZipArchive<File>,
    name: &'static str,
    limit: u64,
    path: &Path,
) -> Result<Vec<u8>, PackageError> {
    let index = entry_index(archive, name)?;
    let entry = archive
        .by_index(index)
        .map_err(PackageError::reading_archive_at(path))?;
    read_within_limit(entry, name, limit, path)
}

/// The SHA-256 of the component of `archive`, the package at `path`, inflated a buffer at a
/// time.
fn component_sha256(archive: &mut ZipArchive<File>, path: &Path) -> Result<[u8; 32], PackageError> {
    let index = entry_index(archive, COMPONENT)?;
    let component = archive
        .by_index(index)
        .map_err(PackageError::reading_archive_at(path))?;
    sha256_of(component, path)
}

fn entry_index(archive: &ZipArchive<File>, name: &'static str) -> Result<usize, PackageError> {
    archive
        .index_for_name(name)
        .ok_or(PackageError::MissingEntry(name))
}

/// Writes `contents` to the file at `path` and waits until they are on disk.
fn write_synced(path: &Path, contents: &[u8]) -> Result<(), PackageError> {
    let write = || {
        let mut file = File::create(path)?;
        std::io::Write::write_all(&mut file, contents)?;
        file.sync_all()
    };
    write().map_err(PackageError::io_at(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{
        TRIANGLE_MANIFEST, component_bytes, png_header, signed, triangle_entries, triangle_package,
        with_entry, without_entry, zip_of,
    };
    use crate::{KeyError, ManifestError, SigningKey};

    #[test]
    fn a_written_package_opens_and_unpacks_to_what_was_written() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let path = triangle_package(scratch.path(), &key);

        let mut package = Package::open(&path).unwrap();
        assert_eq!(package.manifest().id().as_str(), "dev.example.triangle");
        assert_eq!(package.manifest().name(), "Triangle");
        assert_eq!(*package.publisher(), key.publisher());

        let unpacked = scratch.path().join("unpacked");
        package.unpack(&unpacked).unwrap();
        let manifest = fs::read_to_string(unpacked.join(MANIFEST)).unwrap();
        assert_eq!(Manifest::from_toml(&manifest).unwrap(), *package.manifest());
        assert_eq!(
            fs::read(unpacked.join(ICON)).unwrap(),
            png_header(1024, 1024)
        );
        assert_eq!(
            fs::read(unpacked.join(COMPONENT)).unwrap(),
            component_bytes()
        );
        let publisher = fs::read(unpacked.join(PUBLISHER_KEY)).unwrap();
        assert_eq!(
            PublisherKey::from_bytes(&publisher).unwrap(),
            key.publisher()
        );
        assert!(!unpacked.join(SIGNATURE).exists());
    }

    #[test]
    fn a_changed_byte_in_any_signed_entry_is_refused() {
        let entries = signed(triangle_entries(), &SigningKey::generate());
        for changed in [MANIFEST, COMPONENT, ICON] {
            let scratch = tempfile::tempdir().unwrap();
            let mut bytes = entries
                .iter()
                .find(|(name, _)| *name == changed)
                .map(|(_, bytes)| bytes.clone())
                .unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            let path = zip_of(scratch.path(), &with_entry(&entries, changed, &bytes));
            let error = Package::open(&path).unwrap_err();
            assert!(
                matches!(error, PackageError::SignatureDoesNotMatch),
                "{changed}: {error}"
            );
        }
    }

    #[test]
    fn a_package_without_its_signature_or_publisher_key_is_refused() {
        let entries = signed(triangle_entries(), &SigningKey::generate());
        for missing in [SIGNATURE, PUBLISHER_KEY] {
            let scratch = tempfile::tempdir().unwrap();
            let path = zip_of(scratch.path(), &without_entry(&entries, missing));
            let error = Package::open(&path).unwrap_err();
            assert!(
                matches!(error, PackageError::MissingEntry(entry) if entry == missing),
                "{error}"
            );
        }
    }

    #[test]
    fn a_package_signed_under_a_development_installs_id_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let reserved =
            TRIANGLE_MANIFEST.replace("dev.example.triangle", "dev.example.triangle.development");
        let entries = signed(
            with_entry(&triangle_entries(), MANIFEST, reserved.as_bytes()),
            &SigningKey::generate(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(matches!(error, PackageError::DevelopmentId(_)), "{error}");
    }

    #[test]
    fn the_signature_of_another_package_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let key = SigningKey::generate();
        let renamed = TRIANGLE_MANIFEST.replace("Triangle", "Square");
        let other = signed(
            with_entry(&triangle_entries(), MANIFEST, renamed.as_bytes()),
            &key,
        );
        let other_signature = &other.iter().find(|(name, _)| *name == SIGNATURE).unwrap().1;
        let entries = with_entry(
            &signed(triangle_entries(), &key),
            SIGNATURE,
            other_signature,
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(
            matches!(error, PackageError::SignatureDoesNotMatch),
            "{error}"
        );
    }

    #[test]
    fn a_signature_by_a_key_other_than_the_publisher_key_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let signer = SigningKey::generate();
        let another = SigningKey::generate().publisher();
        let entries = with_entry(
            &signed(triangle_entries(), &signer),
            PUBLISHER_KEY,
            &another.to_bytes(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(
            matches!(error, PackageError::SignatureDoesNotMatch),
            "{error}"
        );
    }

    #[test]
    fn a_publisher_key_or_signature_of_another_length_is_refused() {
        let entries = signed(triangle_entries(), &SigningKey::generate());
        let scratch = tempfile::tempdir().unwrap();
        let short_key = with_entry(&entries, PUBLISHER_KEY, &[7; 31]);
        assert!(matches!(
            Package::open(&zip_of(scratch.path(), &short_key)).unwrap_err(),
            PackageError::PublisherKey(KeyError::PublicKeyLength(31))
        ));
        let long_key = with_entry(&entries, PUBLISHER_KEY, &[7; 33]);
        assert!(matches!(
            Package::open(&zip_of(scratch.path(), &long_key)).unwrap_err(),
            PackageError::TooLarge {
                entry: PUBLISHER_KEY,
                limit: 32
            }
        ));
        let short_signature = with_entry(&entries, SIGNATURE, &[7; 63]);
        assert!(matches!(
            Package::open(&zip_of(scratch.path(), &short_signature)).unwrap_err(),
            PackageError::SignatureNotEd25519 { length: 63 }
        ));
    }

    #[test]
    fn each_missing_program_entry_is_named() {
        let entries = signed(triangle_entries(), &SigningKey::generate());
        for missing in [MANIFEST, COMPONENT, ICON] {
            let scratch = tempfile::tempdir().unwrap();
            let path = zip_of(scratch.path(), &without_entry(&entries, missing));
            let error = Package::open(&path).unwrap_err();
            assert!(
                matches!(error, PackageError::MissingEntry(entry) if entry == missing),
                "{error}"
            );
        }
    }

    #[test]
    fn entries_with_other_names_are_ignored() {
        let scratch = tempfile::tempdir().unwrap();
        let mut entries = signed(triangle_entries(), &SigningKey::generate());
        entries.push(("assets/splash.png", b"a later entry".to_vec()));
        let mut package = Package::open(&zip_of(scratch.path(), &entries)).unwrap();
        let unpacked = scratch.path().join("unpacked");
        package.unpack(&unpacked).unwrap();
        assert!(!unpacked.join("assets").exists());
    }

    #[test]
    fn a_manifest_over_its_cap_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let mut padded = TRIANGLE_MANIFEST.as_bytes().to_vec();
        padded.resize(TRIANGLE_MANIFEST.len() + MANIFEST_LIMIT as usize, b'\n');
        let entries = signed(
            with_entry(&triangle_entries(), MANIFEST, &padded),
            &SigningKey::generate(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(
            matches!(
                error,
                PackageError::TooLarge {
                    entry: MANIFEST,
                    limit: MANIFEST_LIMIT
                }
            ),
            "{error}"
        );
        assert_eq!(
            error.to_string(),
            "manifest.toml is larger than 64 KiB, the most a package may hold"
        );
    }

    #[test]
    fn an_icon_over_its_cap_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let mut icon = png_header(1024, 1024);
        icon.resize(ICON_LIMIT as usize + 1, 0);
        let entries = signed(
            with_entry(&triangle_entries(), ICON, &icon),
            &SigningKey::generate(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(
            matches!(
                error,
                PackageError::TooLarge {
                    entry: ICON,
                    limit: ICON_LIMIT
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_signed_icon_that_breaks_the_rules_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let small = png_header(128, 128);
        let entries = signed(
            with_entry(&triangle_entries(), ICON, &small),
            &SigningKey::generate(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(
            matches!(error, PackageError::IconTooSmall { side: 128 }),
            "{error}"
        );
    }

    #[test]
    fn a_signed_manifest_that_is_invalid_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let one_segment = TRIANGLE_MANIFEST.replace("dev.example.triangle", "triangle");
        let entries = signed(
            with_entry(&triangle_entries(), MANIFEST, one_segment.as_bytes()),
            &SigningKey::generate(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(
            matches!(error, PackageError::Manifest(ManifestError::InvalidId(_))),
            "{error}"
        );
    }

    #[test]
    fn a_signed_manifest_that_is_not_utf8_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let latin1 = b"name = \"Caf\xe9\"\n";
        let entries = signed(
            with_entry(&triangle_entries(), MANIFEST, latin1),
            &SigningKey::generate(),
        );
        let error = Package::open(&zip_of(scratch.path(), &entries)).unwrap_err();
        assert!(matches!(error, PackageError::ManifestNotUtf8), "{error}");
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("program.wapk");
        fs::write(&path, component_bytes()).unwrap();
        let error = Package::open(&path).unwrap_err();
        assert!(matches!(error, PackageError::NotAZipArchive(_)), "{error}");
    }

    #[test]
    fn a_damaged_component_is_refused_when_the_package_is_opened() {
        let scratch = tempfile::tempdir().unwrap();
        let path = triangle_package(scratch.path(), &SigningKey::generate());
        damage_component(&path);
        let error = Package::open(&path).unwrap_err();
        assert!(
            matches!(&error, PackageError::Io { path: blamed, .. } if *blamed == path),
            "{error}"
        );
    }

    #[test]
    fn unpacking_refuses_a_component_other_than_the_one_verified() {
        let scratch = tempfile::tempdir().unwrap();
        let path = triangle_package(scratch.path(), &SigningKey::generate());
        let mut package = Package::open(&path).unwrap();
        package.component_sha256 = [0; 32];
        let error = package
            .unpack(&scratch.path().join("unpacked"))
            .unwrap_err();
        assert!(matches!(error, PackageError::ChangedSinceOpened), "{error}");
    }

    /// Overwrites the middle of the component's compressed bytes, which only inflating it
    /// notices.
    fn damage_component(path: &Path) {
        let (start, length) = {
            let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
            let component = archive.by_name(COMPONENT).unwrap();
            (component.data_start().unwrap(), component.compressed_size())
        };
        let mut bytes = fs::read(path).unwrap();
        let middle = (start + length / 2) as usize;
        bytes[middle..middle + 16].fill(0xa5);
        fs::write(path, bytes).unwrap();
    }
}
