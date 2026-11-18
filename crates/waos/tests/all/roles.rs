//! A program's roles, read from what its component imports: one that imports `wasdk:inspect`
//! asks for the inspector role, is asked about at its start while its user has not answered,
//! the answer kept and the question told through its access, and does not start without it, told
//! apart as not granted; granted it, it sees the programs the embedder gives and traces one the
//! engine runs. The inspector is `examples/inspect`, and the program it traces `examples/forever`.

use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use futures::future::{self, BoxFuture};
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos::inspection::bindings::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos::inspection::{InspectedPrograms, ProgramStorage};
use waos::{
    Answer, HeldProgram, OutputStream, ProgramEnd, ProgramEnvironment, ProgramGrants,
    ProgramOutput, Question, Role, RoleNotGranted, TraceKind, WaOSConfig,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

use crate::shell;

/// Grants that keep the user's answer to the inspector role, and nothing else.
#[derive(Default)]
struct RoleGrants {
    inspector: Mutex<Option<Answer>>,
}

impl ProgramGrants for RoleGrants {
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
        *self.inspector.lock().unwrap()
    }

    fn set_role(&self, role: Role, answer: Answer) {
        assert_eq!(role, Role::Inspector);
        *self.inspector.lock().unwrap() = Some(answer);
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(std::future::pending())
    }
}

fn with(grants: &Arc<RoleGrants>) -> ProgramEnvironment {
    ProgramEnvironment {
        grants: Some(Arc::clone(grants) as Arc<dyn ProgramGrants>),
        ..ProgramEnvironment::default()
    }
}

/// The role the program's end says it was not granted, where it was refused for one.
fn not_granted(end: &ProgramEnd) -> Option<Role> {
    match end {
        ProgramEnd::Failed(error) => error.downcast_ref::<RoleNotGranted>().map(|not| not.role()),
        _ => None,
    }
}

#[test]
fn a_program_importing_wasdk_inspect_is_asked_about_at_its_start_and_refused_without_it() {
    let (engine, shell, _arrivals) = shell::engine();
    let inspector = shell::load(&engine, waos_test_programs::IMPORTS_INSPECT);
    let grants = Arc::new(RoleGrants::default());
    let (running, _given) = engine.run(&inspector, with(&grants), Vec::new()).unwrap();
    shell::wait_until("the role asked about", || !shell.roles_asked().is_empty());
    assert_eq!(shell.roles_asked()[0].1, Role::Inspector);
    shell.answer_roles(Some(Answer::Denied));
    let end = block_on(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
    assert_eq!(
        *grants.inspector.lock().unwrap(),
        Some(Answer::Denied),
        "kept"
    );

    let (running, _given) = engine.run(&inspector, with(&grants), Vec::new()).unwrap();
    let end = block_on(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
    assert_eq!(
        shell.roles_asked().len(),
        1,
        "an answer kept is not asked again"
    );
}

#[test]
fn the_role_question_is_told_through_the_programs_access() {
    let (engine, shell, _arrivals) = shell::engine();
    let inspector = shell::load(&engine, waos_test_programs::IMPORTS_INSPECT);
    let grants = Arc::new(RoleGrants::default());
    let (running, _given) = engine.run(&inspector, with(&grants), Vec::new()).unwrap();
    shell::wait_until("the role asked about", || !shell.roles_asked().is_empty());
    let mut permissions = running
        .trace(TraceKind::Permissions)
        .expect("the access is made before the role is checked");
    shell.answer_roles(Some(Answer::Denied));
    let event = block_on(permissions.next()).expect("the answer is told");
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(
        json["kind"]["question-answered"],
        serde_json::json!({"question": "role.inspector", "answer": "deny"})
    );
    block_on(running.ended());
}

#[test]
fn a_program_with_no_grants_holds_no_role() {
    let (engine, shell, _arrivals) = shell::engine();
    let inspector = shell::load(&engine, waos_test_programs::IMPORTS_INSPECT);
    let (running, _given) = engine
        .run(&inspector, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    let end = block_on(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
    assert!(
        shell.roles_asked().is_empty(),
        "nothing could keep the answer"
    );
}

#[test]
fn a_program_granted_its_role_sees_the_programs_and_traces_one() {
    // Forever logs its count, which other tests read in the log.
    let _forever = shell::turns::take("forever");
    let (engine, _shell, _arrivals) = shell::engine_with(WaOSConfig {
        inspected_programs: Some(Arc::new(TwoPrograms)),
        ..shell::config()
    });
    let forever = shell::load(&engine, waos_test_programs::FOREVER);
    let (counting, _given) = engine
        .run(&forever, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    let inspect = shell::load(&engine, waos_test_programs::INSPECT);
    let grants = Arc::new(RoleGrants {
        inspector: Mutex::new(Some(Answer::Granted)),
    });
    let said = Arc::new(Said::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&said) as Arc<dyn ProgramOutput>),
        ..with(&grants)
    };
    let (inspecting, _given) = engine.run(&inspect, environment, Vec::new()).unwrap();
    let end = block_on(inspecting.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    let said = said.text();
    for line in [
        "device Test device this=true",
        "program forever \"Forever\" running=true icon=4 days=0",
        "program inspect \"Inspect\" running=true icon=0 days=0",
        "forever uses 0 now",
        "forever wrote: count ",
    ] {
        assert!(said.contains(line), "{line:?} in {said}");
    }
    counting.stop();
    block_on(counting.ended());
}

/// What an inspector sees in the tests: forever and the inspector, both running, forever with
/// an icon of four bytes.
struct TwoPrograms;

impl InspectedPrograms for TwoPrograms {
    fn device_name(&self) -> String {
        "Test device".to_owned()
    }

    fn programs(&self) -> BoxFuture<'static, Vec<Program>> {
        let program = |id: &str, name: &str| Program {
            id: id.to_owned(),
            name: name.to_owned(),
            version: None,
            running: true,
        };
        future::ready(vec![
            program("forever", "Forever"),
            program("inspect", "Inspect"),
        ])
        .boxed()
    }

    fn program_changes(&self) -> BoxStream<'static, ProgramChange> {
        stream::pending().boxed()
    }

    fn icon(&self, id: &str) -> BoxFuture<'static, Option<Vec<u8>>> {
        future::ready((id == "forever").then(|| vec![0x89, b'P', b'N', b'G'])).boxed()
    }

    fn start_held(&self, _id: &str) -> BoxFuture<'static, Result<HeldProgram, StartError>> {
        future::ready(Err(StartError::Unknown)).boxed()
    }

    fn end(&self, _id: &str) -> BoxFuture<'static, ()> {
        future::ready(()).boxed()
    }

    fn storage(&self, _id: &str) -> Option<ProgramStorage> {
        None
    }
}

/// What a program wrote to its standard output, kept.
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
fn a_program_importing_no_role_is_asked_about_none() {
    let (engine, shell, _arrivals) = shell::engine();
    let incompatible = shell::load(&engine, waos_test_programs::INCOMPATIBLE);
    let (running, _given) = engine
        .run(&incompatible, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    let end = block_on(running.ended());
    assert_eq!(not_granted(&end), None, "{end:?}");
    assert!(shell.roles_asked().is_empty());
}
