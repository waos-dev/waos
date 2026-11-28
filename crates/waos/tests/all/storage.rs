//! What an inspector reads of a program's storage: the folders it sees at paths of its own, and
//! the entries of one of them, resolved through its mounts; and a path that tries to leave its
//! mount, refused. The inspector is `examples/inspect`, which asks the device it sees for the
//! first running program's mounts and reads one.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use futures::future::{self, BoxFuture};
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos::{Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, Question, Role, WaOS};
use waos_capabilities::wit::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos_capabilities::{
    AppFiles, AppHold, ChangedFuture, DomainRule, FileSystemGrant, InspectedApps, NetworkGrant,
    ProgramFolders,
};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

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

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn role(&self, role: Role) -> Option<Answer> {
        assert_eq!(role, Role::Inspector);
        Some(Answer::Granted)
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(std::future::pending())
    }
}

/// The programs an inspector sees here: one running, with its own folders.
struct StoragePrograms {
    folders: ProgramFolders,
}

impl InspectedApps for StoragePrograms {
    fn device_name(&self) -> String {
        "Test device".to_owned()
    }

    fn apps(&self) -> BoxFuture<'static, Vec<Program>> {
        future::ready(vec![Program {
            id: "dev.example.storage".to_owned(),
            name: "Storage".to_owned(),
            version: None,
            running: true,
        }])
        .boxed()
    }

    fn app_changes(&self) -> BoxStream<'static, ProgramChange> {
        stream::pending().boxed()
    }

    fn icon(&self, _id: &str) -> BoxFuture<'static, Option<Vec<u8>>> {
        future::ready(None).boxed()
    }

    fn start_held(&self, _id: &str) -> BoxFuture<'static, Result<AppHold, StartError>> {
        future::ready(Err(StartError::Unknown)).boxed()
    }

    fn end(&self, _id: &str) -> BoxFuture<'static, ()> {
        future::ready(()).boxed()
    }

    fn files(&self, id: &str) -> Option<AppFiles> {
        (id == "dev.example.storage").then(|| AppFiles {
            folders: Some(self.folders.clone()),
            grants: None,
        })
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

    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let inspect = engine
        .load(
            Path::new(waos_test_programs::INSPECT),
            &waos_test_programs::code_of(waos_test_programs::INSPECT),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::new(Granted)),
        inspected: Some(Arc::new(StoragePrograms { folders })),
        ..printing_to(&output)
    };
    let running = engine.run(&inspect, environment).unwrap();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");

    let said = output.lines().join("\n");
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
