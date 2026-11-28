//! The sources the user added, kept in `sources.toml` at the top of WaOS's store directory
//! ([`SourcesFile`]) as the text each source shows as, in the order they were added:
//!
//! ```toml
//! sources = ["github.com/example/triangle", "example.com"]
//! ```
//!
//! The invariants: the file, once made, is the list, whatever it holds; an empty list is the
//! user's, never a reason to make it again. And this is the file and the place WaOS has always
//! kept the sources in, which people have; a change of either loses what they added.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::files::write_whole;
use crate::sources::{Source, SourcesStore};

/// The file of sources, in the directory.
const SOURCES: &str = "sources.toml";

/// The list of sources as a file in a directory, WaOS's own.
pub struct SourcesFile {
    path: PathBuf,
}

impl SourcesFile {
    /// The list in `directory`.
    pub fn new(directory: &Path) -> SourcesFile {
        SourcesFile {
            path: directory.join(SOURCES),
        }
    }
}

impl SourcesStore for SourcesFile {
    fn load(&self) -> io::Result<Option<Vec<Source>>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(at(&self.path)(error)),
        };
        let file: ListFile = toml::from_str(&text).map_err(unreadable_at(&self.path))?;
        let sources = file.sources.iter().map(|source| source.parse());
        sources
            .collect::<Result<_, _>>()
            .map(Some)
            .map_err(unreadable_at(&self.path))
    }

    fn save(&self, sources: &[Source]) -> io::Result<()> {
        let folder = self.path.parent().expect("the file is in the directory");
        fs::create_dir_all(folder).map_err(at(folder))?;
        let file = ListFile {
            sources: sources.iter().map(Source::to_string).collect(),
        };
        let text = toml::to_string(&file).expect("a list of strings is TOML");
        write_whole(&self.path, text.as_bytes()).map_err(at(&self.path))
    }
}

/// The file as TOML spells it.
#[derive(Default, Serialize, Deserialize)]
struct ListFile {
    sources: Vec<String>,
}

/// For `map_err`: an IO error at `path`, said with it.
fn at(path: &Path) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |error| io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// For `map_err`: the file at `path` is not as WaOS writes it, for the reason `error` gives.
fn unreadable_at<E: std::fmt::Display>(path: &Path) -> impl FnOnce(E) -> io::Error + '_ {
    move |error| {
        let said = format!("{} is not as WaOS wrote it: {error}", path.display());
        io::Error::new(ErrorKind::InvalidData, said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_read_back_in_the_order_written() {
        let scratch = tempfile::tempdir().unwrap();
        let file = SourcesFile::new(scratch.path());
        assert_eq!(file.load().unwrap(), None);
        let sources: Vec<Source> = ["example.com", "github.com/example/triangle"]
            .iter()
            .map(|text| text.parse().unwrap())
            .collect();
        file.save(&sources).unwrap();
        assert_eq!(
            SourcesFile::new(scratch.path()).load().unwrap(),
            Some(sources)
        );
    }

    #[test]
    fn a_list_emptied_is_made_still() {
        let scratch = tempfile::tempdir().unwrap();
        let file = SourcesFile::new(scratch.path());
        file.save(&[]).unwrap();
        assert_eq!(file.load().unwrap(), Some(Vec::new()));
    }

    #[test]
    fn a_file_that_is_not_a_list_of_sources_is_unreadable() {
        let scratch = tempfile::tempdir().unwrap();
        let file = SourcesFile::new(scratch.path());
        for text in ["sources = 7", "sources = [\"ftp://example.com\"]"] {
            fs::write(scratch.path().join(SOURCES), text).unwrap();
            let error = file.load().unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidData, "{error}");
        }
    }
}
