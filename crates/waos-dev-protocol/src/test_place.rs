//! Where a WaOS that is not the user's listens: a test's, apart from every other test's and from
//! the user's own WaOS. On Unix a socket in a folder of its own, removed with it; on Windows a pipe
//! of its own name. Tests of either side of the protocol listen here instead of at
//! [`listener_path`](crate::listener_path), so the difference between the two systems' transports
//! stays in this crate.

use std::path::{Path, PathBuf};

/// The place a test's WaOS listens at, named after the test; on Unix, its folder goes when this
/// is dropped.
#[derive(Debug)]
pub struct TestPlace {
    path: PathBuf,
}

impl TestPlace {
    /// A place for the test `test`, apart from this process's other tests by their names and from
    /// other processes by this one's id.
    #[cfg(unix)]
    pub fn new(test: &str) -> TestPlace {
        let folder = std::env::temp_dir().join(format!("waos-dev-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        TestPlace {
            path: folder.join("dev.sock"),
        }
    }

    /// A pipe for the test `test`, apart from this process's other tests by their names and from
    /// other processes by this one's id.
    #[cfg(windows)]
    pub fn new(test: &str) -> TestPlace {
        TestPlace {
            path: PathBuf::from(format!(
                r"\\.\pipe\waos-dev-test-{}-{test}",
                std::process::id()
            )),
        }
    }

    /// Where [`Listener::bind`](crate::Listener::bind) listens and
    /// [`Connection::connect`](crate::Connection::connect) reaches it.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for TestPlace {
    fn drop(&mut self) {
        if let Some(folder) = self.path.parent() {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}
