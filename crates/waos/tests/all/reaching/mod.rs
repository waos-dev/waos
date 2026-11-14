//! What the tests of a program's network share: grants of the network and the local network,
//! which a test sets as the user would, and a program run with them, what it is to reach written
//! in its `/data`, and what it prints.

use std::fs;
use std::future::pending;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::executor::block_on;
use tempfile::TempDir;
use waos::{
    Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, Question, RunningProgram,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant, ProgramFolders};

use crate::shell::{self, Lines};

pub fn any() -> NetworkGrant {
    NetworkGrant {
        on: true,
        domains: DomainRule::Any,
    }
}

pub fn off() -> NetworkGrant {
    NetworkGrant {
        on: false,
        domains: DomainRule::Any,
    }
}

pub fn only(names: &[&str]) -> NetworkGrant {
    NetworkGrant {
        on: true,
        domains: DomainRule::Only(names.iter().map(|name| name.parse().unwrap()).collect()),
    }
}

/// Grants of nothing but the network and the local network.
pub struct TestGrants {
    network: Mutex<NetworkGrant>,
    local_network: Mutex<bool>,
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
}

impl TestGrants {
    pub fn new(network: NetworkGrant, local_network: bool) -> Arc<TestGrants> {
        Arc::new(TestGrants {
            network: Mutex::new(network),
            local_network: Mutex::new(local_network),
            waiting: Mutex::default(),
        })
    }

    /// The user turns the local network on or off.
    pub fn set_local_network(&self, on: bool) {
        *self.local_network.lock().unwrap() = on;
        for waiting in self.waiting.lock().unwrap().drain(..) {
            let _ = waiting.send(());
        }
    }
}

impl ProgramGrants for TestGrants {
    fn network(&self) -> NetworkGrant {
        self.network.lock().unwrap().clone()
    }

    fn local_network(&self) -> bool {
        *self.local_network.lock().unwrap()
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        let (changed, heard) = oneshot::channel();
        self.waiting.lock().unwrap().push(changed);
        Box::pin(async move {
            if heard.await.is_err() {
                pending::<()>().await;
            }
        })
    }
}

/// The program's `/data/port`: where its test's server answers.
pub fn port_of(port: u16) -> [(&'static str, Vec<u8>); 1] {
    [("port", port.to_string().into_bytes())]
}

/// A program running with grants of the test's, and what it writes.
pub struct Run {
    running: RunningProgram,
    pub lines: Arc<Lines>,
    /// Its own folders, gone with the run.
    _folders: TempDir,
}

impl Run {
    /// Runs the program at `component` with `grants`, each of `data`'s files written in its
    /// `/data` under its name.
    pub fn start(component: &str, grants: &Arc<TestGrants>, data: &[(&str, Vec<u8>)]) -> Run {
        let folders = TempDir::new().unwrap();
        let data_folder = folders.path().join("data");
        fs::create_dir_all(&data_folder).unwrap();
        for (name, contents) in data {
            fs::write(data_folder.join(name), contents).unwrap();
        }
        let data = data_folder;
        let (engine, _shell, _arrivals) = shell::engine();
        let program = shell::load(&engine, component);
        let lines = Arc::new(Lines::default());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
            grants: Some(Arc::clone(grants) as Arc<dyn ProgramGrants>),
            folders: Some(ProgramFolders {
                data,
                cache: folders.path().join("cache"),
            }),
            ..ProgramEnvironment::default()
        };
        let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
        Run {
            running,
            lines,
            _folders: folders,
        }
    }

    /// Waits until the program returned, failing when it ended otherwise.
    pub fn returned(&self) {
        let end = block_on(self.running.ended());
        assert!(
            matches!(end, ProgramEnd::Returned(Ok(()))),
            "{end:?}: {:#?}",
            self.lines.all()
        );
    }
}
