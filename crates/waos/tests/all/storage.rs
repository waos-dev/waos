//! What an inspector reads of a program's storage: the folders it sees at paths of its own, and
//! the entries of one of them, resolved through its mounts; and a path that tries to leave its
//! mount, refused. The inspector is `examples/inspect`, which asks the device it sees for the
//! first running program's mounts and reads one.

use std::fs;
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use futures::future::{self, BoxFuture};
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos::inspection::bindings::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos::inspection::{InspectedPrograms, ProgramStorage};
use waos::{
    Answer, HeldProgram, OutputStream, ProgramEnd, ProgramEnvironment, ProgramGrants,
    ProgramOutput, Question, Role, WaOSConfig,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant, ProgramFolders};

use crate::shell;

/// Grants that answer the inspector role with yes and let nothing else.
struct Granted;

impl ProgramGrants for Granted {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Any,
        }
    }

    fn local_network(&self) -> bool {
        false
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

    fn set_answer(&self, _question: Question, _answer: Answer) {
        panic!("the program asks no question");
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {
        panic!("the program opens no link");
    }

    fn role(&self, role: Role) -> Option<Answer> {
        assert_eq!(role, Role::Inspector);
        Some(Answer::Granted)
    }

    fn set_role(&self, _role: Role, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        Box::pin(std::future::pending())
    }
}

/// The programs an inspector sees here: one running, with its own folders.
struct StoragePrograms {
    folders: ProgramFolders,
}

impl InspectedPrograms for StoragePrograms {
    fn device_name(&self) -> String {
        "Test device".to_owned()
    }

    fn programs(&self) -> BoxFuture<'static, Vec<Program>> {
        future::ready(vec![Program {
            id: "dev.example.storage".to_owned(),
            name: "Storage".to_owned(),
            version: None,
            running: true,
        }])
        .boxed()
    }

    fn program_changes(&self) -> BoxStream<'static, ProgramChange> {
        stream::pending().boxed()
    }

    fn icon(&self, _id: &str) -> BoxFuture<'static, Option<Vec<u8>>> {
        future::ready(None).boxed()
    }

    fn start_held(&self, _id: &str) -> BoxFuture<'static, Result<HeldProgram, StartError>> {
        future::ready(Err(StartError::Unknown)).boxed()
    }

    fn end(&self, _id: &str) -> BoxFuture<'static, ()> {
        future::ready(()).boxed()
    }

    fn storage(&self, id: &str) -> Option<ProgramStorage> {
        (id == "dev.example.storage")
            .then(|| ProgramStorage::of(Some(self.folders.clone()), None, None))
    }
}

/// What the program wrote, kept.
#[derive(Default)]
struct Said(Mutex<Vec<u8>>);

impl Said {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl ProgramOutput for Said {
    fn poll_write(
        &self,
        _stream: OutputStream,
        _cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        std::task::Poll::Ready(bytes.len())
    }
}

#[test]
fn an_inspector_reads_a_programs_mounts_and_a_folders_entries_and_no_path_out_of_one() {
    let folder = tempfile::tempdir().unwrap();
    let data = folder.path().join("data");
    let cache = folder.path().join("cache");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::write(data.join("notes.txt"), b"abc").unwrap();
    fs::create_dir(data.join("sub")).unwrap();
    let folders = ProgramFolders {
        data: data.clone(),
        cache: cache.clone(),
    };

    let (engine, _shell, _arrivals) = shell::engine_with(WaOSConfig {
        inspected_programs: Some(Arc::new(StoragePrograms { folders })),
        ..shell::config()
    });
    let inspect = shell::load(&engine, waos_test_programs::INSPECT);
    let said = Arc::new(Said::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&said) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::new(Granted)),
        ..ProgramEnvironment::default()
    };
    let running = engine.run(&inspect, environment).unwrap();
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");

    let said = said.text();
    assert!(
        said.contains(&format!(
            "mount /data MountKind::OwnData writable=true at {}",
            data.display()
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "mount /cache MountKind::OwnCache writable=true at {}",
            cache.display()
        )),
        "{said}"
    );
    assert!(
        said.contains("entry sub folder=true size=None modified=true"),
        "a folder has no size, and the system keeps its time: {said}"
    );
    assert!(
        said.contains("entry notes.txt folder=false size=Some(3) modified=true"),
        "a file's size and time: {said}"
    );
    assert!(
        said.contains("escape refused: not permitted"),
        "a path out of its mount is refused: {said}"
    );
    assert!(!said.contains("escape read"), "{said}");
}
