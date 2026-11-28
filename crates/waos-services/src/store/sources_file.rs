//! The sources the user added, kept in `sources.toml` as the text each source shows as, in the
//! order they were added:
//!
//! ```toml
//! sources = ["github.com/example/triangle", "example.com"]
//! ```
//!
//! The invariant: the file, once made, is the list, whatever it holds; an empty list is the
//! user's, never a reason to make it again.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::store::files::write_whole;
use crate::store::{Source, StoreError};

/// The file as TOML spells it.
#[derive(Default, Serialize, Deserialize)]
struct SourcesFile {
    sources: Vec<String>,
}

/// The sources in the file at `path`; none when there is no file.
pub(crate) fn read(path: &Path) -> Result<Vec<Source>, StoreError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StoreError::io_at(path)(error)),
    };
    let file: SourcesFile = toml::from_str(&text).map_err(StoreError::unreadable_at(path))?;
    file.sources
        .iter()
        .map(|source| source.parse().map_err(StoreError::unreadable_at(path)))
        .collect()
}

/// Whether the list was ever made: whether there is a file at `path`.
pub(crate) fn is_made(path: &Path) -> Result<bool, StoreError> {
    fs::exists(path).map_err(StoreError::io_at(path))
}

/// Writes `sources` to the file at `path`, whole.
pub(crate) fn write(path: &Path, sources: &[Source]) -> Result<(), StoreError> {
    let file = SourcesFile {
        sources: sources.iter().map(Source::to_string).collect(),
    };
    let text = toml::to_string(&file).expect("a list of strings is TOML");
    write_whole(path, text.as_bytes()).map_err(StoreError::io_at(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_read_back_in_the_order_written() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("sources.toml");
        assert!(read(&path).unwrap().is_empty());
        let sources: Vec<Source> = ["example.com", "github.com/example/triangle"]
            .iter()
            .map(|text| text.parse().unwrap())
            .collect();
        write(&path, &sources).unwrap();
        assert_eq!(read(&path).unwrap(), sources);
    }

    #[test]
    fn a_list_emptied_is_made_still() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("sources.toml");
        assert!(!is_made(&path).unwrap());
        write(&path, &[]).unwrap();
        assert!(is_made(&path).unwrap());
    }

    #[test]
    fn a_file_that_is_not_a_list_of_sources_is_unreadable() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("sources.toml");
        for text in ["sources = 7", "sources = [\"ftp://example.com\"]"] {
            fs::write(&path, text).unwrap();
            let error = read(&path).unwrap_err();
            assert!(matches!(error, StoreError::Unreadable { .. }), "{error}");
        }
    }
}
