//! What the user opened with a program: files and links, as the embedder hears them from the
//! system or from WaOS's command line, and as the program hears them on `open-requests`.
//!
//! The invariant: a file reaches the program only mounted, writable at its own path, as the user
//! chose to open it in this program; it is mounted as the program reads the request, so the path it
//! reads is one it reaches.

use std::path::PathBuf;

use crate::bindings::wasdk::platform::lifecycle;
use crate::files::Picks;

/// What the user opened with a program: files, by their paths on the machine, and links.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenRequest {
    pub files: Vec<PathBuf>,
    pub links: Vec<String>,
}

impl OpenRequest {
    /// The request as the program hears it, each file mounted through `picks`, writable at its
    /// own path, and kept among the program's mounts as a pick is; a file a program has no path
    /// for is left out, said so.
    pub(crate) fn mounted(self, picks: &Picks) -> lifecycle::OpenRequest {
        lifecycle::OpenRequest {
            files: self
                .files
                .iter()
                .filter_map(|file| picks.mount(file, true))
                .collect(),
            links: self.links,
        }
    }
}
