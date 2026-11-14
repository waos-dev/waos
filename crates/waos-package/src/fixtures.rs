//! What the tests make packages from: a valid manifest, component and icon, their signature by
//! any key, and zips of whatever entries a test needs.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::entries::{COMPONENT, ICON, MANIFEST, PUBLISHER_KEY, SIGNATURE};
use crate::signed_message::{SignedDigests, sha256};
use crate::{Manifest, SigningKey, write};

/// A package's entries, by name.
pub(crate) type Entries = Vec<(&'static str, Vec<u8>)>;

pub(crate) const TRIANGLE_MANIFEST: &str =
    "id = \"dev.example.triangle\"\nname = \"Triangle\"\nversion = \"0.1.0\"\n";

/// Writes `contents` to the file `name` in `directory` and returns its path.
pub(crate) fn write_file(directory: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// The start of a PNG of `width` by `height`: its signature and IHDR chunk, all that an
/// icon's size is read from.
pub(crate) fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(&[0; 4]);
    png
}

/// A component's preamble followed by 64 KiB that do not compress away, so its compressed
/// bytes have a middle to damage.
pub(crate) fn component_bytes() -> Vec<u8> {
    let mut state: u32 = 0x2545_f491;
    let noise = (0..64 * 1024).map(|_| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state.to_le_bytes()[0]
    });
    b"\0asm\x0d\0\x01\0".iter().copied().chain(noise).collect()
}

/// The program's entries of a valid package, unsigned.
pub(crate) fn triangle_entries() -> Entries {
    vec![
        (MANIFEST, TRIANGLE_MANIFEST.as_bytes().to_vec()),
        (COMPONENT, component_bytes()),
        (ICON, png_header(1024, 1024)),
    ]
}

/// `entries` with the publisher key of `key` and its signature over the manifest, component and
/// icon among them, whatever those hold, as a publisher signing a broken package would make it.
pub(crate) fn signed(mut entries: Entries, key: &SigningKey) -> Entries {
    let digest_of = |name: &str| {
        let (_, bytes) = entries.iter().find(|(entry, _)| *entry == name).unwrap();
        sha256(bytes)
    };
    let signed = SignedDigests {
        manifest: digest_of(MANIFEST),
        component: digest_of(COMPONENT),
        icon: digest_of(ICON),
    };
    let signature = key.sign(signed.message().as_bytes());
    entries.push((PUBLISHER_KEY, key.publisher().to_bytes().to_vec()));
    entries.push((SIGNATURE, signature.to_bytes().to_vec()));
    entries
}

/// `entries` with the entry `name` holding `bytes` instead.
pub(crate) fn with_entry(entries: &Entries, name: &'static str, bytes: &[u8]) -> Entries {
    let mut changed = without_entry(entries, name);
    changed.push((name, bytes.to_vec()));
    changed
}

/// `entries` without the entry `name`.
pub(crate) fn without_entry(entries: &Entries, name: &str) -> Entries {
    let mut kept = entries.clone();
    kept.retain(|(entry, _)| *entry != name);
    kept
}

/// A package written by [`write`] from the triangle's entries, signed with `key`.
pub(crate) fn triangle_package(directory: &Path, key: &SigningKey) -> PathBuf {
    let manifest = Manifest::from_toml(TRIANGLE_MANIFEST).unwrap();
    let component = write_file(directory, "triangle.wasm", &component_bytes());
    let icon = write_file(directory, "icon.png", &png_header(1024, 1024));
    let path = directory.join("triangle.wapk");
    write(&path, &manifest, &component, &icon, key).unwrap();
    path
}

/// A zip of `entries` in `directory`, written without [`write`]'s checks, as a hostile or
/// broken package would be.
pub(crate) fn zip_of(directory: &Path, entries: &[(&str, Vec<u8>)]) -> PathBuf {
    let path = directory.join("package.wapk");
    let mut archive = ZipWriter::new(File::create(&path).unwrap());
    for (name, bytes) in entries {
        archive
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.finish().unwrap();
    path
}
