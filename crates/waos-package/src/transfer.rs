//! Moving an entry's bytes between a package and a file: read whole within a cap, or streamed and
//! hashed on the way, an error blamed on the file it happened in.

use std::io::{self, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::PackageError;

/// Reads all of `entry` from `reader`, the file at `path`, refusing it once it passes `limit`
/// bytes: the size an archive claims for an entry is not trusted.
pub(crate) fn read_within_limit(
    reader: impl Read,
    entry: &'static str,
    limit: u64,
    path: &Path,
) -> Result<Vec<u8>, PackageError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(PackageError::io_at(path))?;
    check_within_limit(entry, &bytes, limit)?;
    Ok(bytes)
}

/// Refuses `entry` if `bytes` are more than `limit`.
pub(crate) fn check_within_limit(
    entry: &'static str,
    bytes: &[u8],
    limit: u64,
) -> Result<(), PackageError> {
    if bytes.len() as u64 > limit {
        return Err(PackageError::TooLarge { entry, limit });
    }
    Ok(())
}

/// Copies `from`, the file at `from_path`, into `to`, the file at `to_path`, a buffer at a time,
/// and answers the SHA-256 of what it copied.
pub(crate) fn stream(
    mut from: impl Read,
    from_path: &Path,
    mut to: impl Write,
    to_path: &Path,
) -> Result<[u8; 32], PackageError> {
    let mut sha256 = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(sha256.finalize().into()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(PackageError::io_at(from_path)(error)),
        };
        sha256.update(&buffer[..read]);
        to.write_all(&buffer[..read])
            .map_err(PackageError::io_at(to_path))?;
    }
}

/// The SHA-256 of all of `from`, the file at `path`, read a buffer at a time.
pub(crate) fn sha256_of(from: impl Read, path: &Path) -> Result<[u8; 32], PackageError> {
    // Writing to the sink never fails, so no error is blamed on the second path.
    stream(from, path, io::sink(), path)
}
