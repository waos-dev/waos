//! Where the paths of the files open in Notes are kept from one launch to the next: in the
//! program's `/data/open-files.json`, written safely, its draft beside it. WaOS keeps each file the
//! user picked or opened the program with mounted at its own path among the program's grants, so
//! a path kept reaches the same file at the next launch.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::safe_write::write_safely;

/// The file the paths are kept in, in the program's own folder.
const FILE: &str = "/data/open-files.json";

#[derive(Debug)]
pub(crate) struct OpenFileList {
    path: PathBuf,
}

/// The list's file.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ListFile {
    files: Vec<PathBuf>,
}

impl OpenFileList {
    /// The list in the program's `/data`.
    pub(crate) fn in_data() -> OpenFileList {
        OpenFileList::at(PathBuf::from(FILE))
    }

    /// The list kept at `path`.
    pub(crate) fn at(path: PathBuf) -> OpenFileList {
        OpenFileList { path }
    }

    /// The paths kept, in their order; none before the first is kept.
    pub(crate) fn load(&self) -> io::Result<Vec<PathBuf>> {
        let json = match fs::read_to_string(&self.path) {
            Ok(json) => json,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let list: ListFile = serde_json::from_str(&json)?;
        Ok(list.files)
    }

    /// Keeps `files`, in their order.
    pub(crate) fn save(&self, files: Vec<PathBuf>) -> io::Result<()> {
        let json = serde_json::to_string(&ListFile { files }).map_err(io::Error::other)?;
        let folder = self.path.parent().unwrap_or(&self.path);
        write_safely(&self.path, &json, folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paths_kept_load_back_in_their_order() {
        let folder = tempfile::tempdir().unwrap();
        let list = OpenFileList::at(folder.path().join("open-files.json"));
        assert!(list.load().unwrap().is_empty(), "none kept yet");
        let files = vec![PathBuf::from("/Users/me/b.txt"), PathBuf::from("/a.txt")];
        list.save(files.clone()).unwrap();
        assert_eq!(list.load().unwrap(), files);
    }
}
