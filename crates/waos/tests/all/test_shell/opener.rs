//! The shell's opener, as the system's would answer it: each call recorded, as it was asked, and
//! answered at once, opened, or with no app for what a test said none opens.

use std::future::ready;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use waos::bindings::wasdk::platform::opener::OpenError;
use waos::{AppInstance, OpenFuture, ShellOpener};
use waos_headless::testing::Changes;

/// What was opened, and what no app opens.
#[derive(Default)]
pub struct TestOpener {
    opened: Mutex<Vec<Opened>>,
    /// The links, and the paths on the machine, that no app opens.
    without_app: Mutex<Vec<String>>,
    changes: Arc<Changes>,
}

/// One call, as a program made it.
#[derive(Clone, Debug, PartialEq)]
pub enum Opened {
    Link { program: String, link: String },
    File { program: String, path: PathBuf },
    Revealed { program: String, path: PathBuf },
}

impl TestOpener {
    /// An opener asked nothing, announcing each call to `changes`.
    pub fn announcing(changes: &Arc<Changes>) -> TestOpener {
        TestOpener {
            changes: Arc::clone(changes),
            ..TestOpener::default()
        }
    }

    /// No app opens `link`, or the file at `path` on the machine, from now on.
    pub fn without_app_for(&self, link_or_path: &str) {
        self.without_app
            .lock()
            .unwrap()
            .push(link_or_path.to_owned());
    }

    /// Each call made so far, in order.
    pub fn opened(&self) -> Vec<Opened> {
        self.opened.lock().unwrap().clone()
    }

    /// Keeps `call`, and answers it: opened, unless no app opens `what`.
    fn answer(&self, call: Opened, what: &str) -> OpenFuture {
        self.opened.lock().unwrap().push(call);
        self.changes.announce();
        let has_app = !self
            .without_app
            .lock()
            .unwrap()
            .iter()
            .any(|none| none == what);
        Box::pin(ready(if has_app {
            Ok(())
        } else {
            Err(OpenError::NoApp)
        }))
    }
}

impl ShellOpener for TestOpener {
    fn open_link(&self, program: &AppInstance, link: &str) -> OpenFuture {
        let call = Opened::Link {
            program: program.name().to_owned(),
            link: link.to_owned(),
        };
        self.answer(call, link)
    }

    fn open_file(&self, program: &AppInstance, path: &Path) -> OpenFuture {
        let call = Opened::File {
            program: program.name().to_owned(),
            path: path.to_owned(),
        };
        self.answer(call, &path.to_string_lossy())
    }

    fn reveal_file(&self, program: &AppInstance, path: &Path) -> OpenFuture {
        let call = Opened::Revealed {
            program: program.name().to_owned(),
            path: path.to_owned(),
        };
        self.answer(call, &path.to_string_lossy())
    }
}
