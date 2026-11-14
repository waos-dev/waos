# waos-package

The `.wapk` format, the form a program is distributed in, signed with its publisher's key; and `index.json`, the list a package source offers its programs in. A raw `.wasm` component is for development. It is the format and nothing else: what is installed where is `waos-store`'s, and where a publisher keeps their signing key is `waos-cli`'s.

- **Takes:** a package file to open, verify or unpack; a manifest, a component file, an icon file and the publisher's signing key to write one from; an index's JSON to read.
- **Provides:** `Package`, an open package whose signature verified and whose manifest and icon were checked, which answers its manifest, its icon and its publisher, unpacked to a directory with its component streamed to disk; `write`, which makes and signs a package; `ICON_MINIMUM_SIDE`, the smallest side an icon may have, 256 pixels; `Manifest`, and `ProgramId`, a reverse-DNS id in the form both an Apple bundle id and an Android application id accept; `PublisherKey`, the public key a package carries, with a short fingerprint for people, and `SigningKey`, the private key that signs, made from the system's random source and kept as PEM text; `Index` and `ListedProgram`, a source's `index.json`; and `PackageError`, `ManifestError`, `IdError`, `KeyError` and `IndexError`, which say what is wrong in plain words.
- **Used by:** `waos-cli`, whose `cargo waos package` writes a package and lists it in an index; `waos-store`, which reads sources' indexes and installs packages; the WaOS app, whose library opens a `.wapk` opened with WaOS to say what it is before installing it, and shows what a source lists. It names no other crate of ours.

## A package

A zip whose entries are all deflated:

```text
triangle.wapk
  manifest.toml    id, name, version
  program.wasm     the program, as a component
  icon.png         a square PNG at least 256 pixels on a side; 1024 recommended, since macOS icons want it
  publisher.key    the publisher's Ed25519 public key: its 32 bytes
  signature        the publisher's Ed25519 signature: its 64 bytes
```

The manifest is TOML:

```toml
id = "dev.example.triangle"
name = "Triangle"
version = "1.2.0"
```

An id is two or more segments joined by dots, each of lowercase ASCII letters and digits, starting with a letter. The version is semver. Fields a later version adds are ignored.

Entries with other names are outside the signature and ignored, so a later version can add its own. Reading holds no more of an entry in memory than its cap, whatever size the archive claims for it: 64 KiB for the manifest, 8 MiB for the icon, and the exact size of the key and of the signature; the component is never held whole.

## The signature

The signature is Ed25519 (RFC 8032), checked with ed25519-dalek's `verify_strict`, over this UTF-8 text: a first line naming the form, then a line for each of the three entries in this order, its SHA-256 as 64 lowercase hex digits, two spaces, and its name, every line ending with a line feed.

```text
wapk-signature-1
<SHA-256 of manifest.toml>  manifest.toml
<SHA-256 of program.wasm>  program.wasm
<SHA-256 of icon.png>  icon.png
```

The lines after the first are what `shasum -a 256 manifest.toml program.wasm icon.png` prints in a folder the package was unzipped into. The message depends on the entries' bytes alone, so packing them again keeps the signature (the design's decision 44). A package proves itself wherever it came from: `publisher.key` says whose it claims to be, and the signature that the publisher's private key made it. Whether that publisher may install or update a program is the installer's rule, not the format's.

`Package::open` verifies the signature before anything the package says is believed: it reads the manifest and icon within their caps, inflates the component once for its SHA-256, and checks the signature with the package's `publisher.key`; only then does it check the manifest and the icon. `Package::unpack` writes `manifest.toml`, `icon.png`, `publisher.key` and `program.wasm` into a directory, streaming the component again and refusing it unless it is the one `open` verified; each file is on disk, not only in the system's cache, when it returns.

## The keys

A `SigningKey` is a publisher's private key, made once from the system's random source. Its text is PKCS#8's PEM, the standard form of an Ed25519 private key (RFC 8410), which begins `-----BEGIN PRIVATE KEY-----`; `openssl genpkey -algorithm ed25519` makes the same form. A `PublisherKey` is its public half. Its fingerprint, for people to tell keys apart by in the library and in errors, is the first 8 bytes of the key's SHA-256 as four groups of four hex digits, like `E744 C079 1320 C328`; installers compare whole keys.

## An index

A package source's `index.json` lists each program it offers at each version, with its package's URL, relative to the index's own, and the package file's SHA-256 (the design's decision 45):

```json
{
  "programs": [
    {
      "id": "dev.example.triangle",
      "name": "Triangle",
      "version": "1.2.0",
      "package": "dev.example.triangle-1.2.0.wapk",
      "sha256": "<the package file's SHA-256, 64 hex digits>"
    }
  ]
}
```

An index is not signed: each package is, and the SHA-256 ties what is downloaded to what the index lists. Listing a program at a version it already lists replaces that listing. Fields a later version adds are ignored.

```rust
use std::path::Path;

use waos_package::{Manifest, Package, SigningKey, Version};

let key = SigningKey::generate();
let manifest = Manifest::new("dev.example.triangle".parse()?, "Triangle", Version::new(1, 2, 0))?;
waos_package::write(
    Path::new("triangle.wapk"),
    &manifest,
    Path::new("triangle.wasm"),
    Path::new("icon.png"),
    &key,
)?;

let mut package = Package::open(Path::new("triangle.wapk"))?;
println!("{} {}, published by {}", package.manifest().name(), package.manifest().version(), package.publisher().fingerprint());
package.unpack(Path::new("programs/dev.example.triangle"))?;
```
