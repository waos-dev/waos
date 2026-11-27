//! A program's roles, read from what its component imports: one that imports `wasdk:inspect`
//! asks for the inspector role, is asked about at its start while its user has not answered,
//! the answer kept and the question told through its access, and does not start without it, told
//! apart as not granted; granted it, it sees the programs the embedder gives and traces one the
//! engine runs. The inspector is `examples/inspect`, and the program it traces `examples/forever`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use futures::future::{self, BoxFuture};
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use tokio::sync::Notify;
use waos::inspection::bindings::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos::inspection::{InspectedPrograms, ProgramStorage};
use waos::{
    Answer, HeldProgram, ProgramEnd, ProgramEnvironment, ProgramGrants, Question, Role,
    RoleNotGranted, TraceKind, WaOS, WaOSConfig,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// Grants that keep the user's answer to the inspector role, and nothing else.
#[derive(Default)]
struct RoleGrants {
    inspector: Mutex<Option<Answer>>,
    changed: Arc<Notify>,
}

impl RoleGrants {
    fn set(&self, answer: Answer) {
        *self.inspector.lock().unwrap() = Some(answer);
        self.changed.notify_waiters();
    }
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
        Box::pin(Arc::clone(&self.changed).notified_owned())
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
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let inspector = engine
        .load(
            Path::new(waos_test_programs::IMPORTS_INSPECT),
            &waos_test_programs::code_of(waos_test_programs::IMPORTS_INSPECT),
        )
        .unwrap();
    let grants = Arc::new(RoleGrants::default());
    let running = engine.run(&inspector, with(&grants)).unwrap();
    wait_until("the role asked about", || {
        !shell.prompts.roles_asked().is_empty()
    });
    assert_eq!(shell.prompts.roles_asked()[0].1, Role::Inspector);
    shell.prompts.answer_roles(Some(Answer::Denied));
    let end = within(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
    assert_eq!(
        *grants.inspector.lock().unwrap(),
        Some(Answer::Denied),
        "kept"
    );

    let running = engine.run(&inspector, with(&grants)).unwrap();
    let end = within(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
    assert_eq!(
        shell.prompts.roles_asked().len(),
        1,
        "an answer kept is not asked again"
    );
}

#[test]
fn the_role_question_is_told_through_the_programs_access() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let inspector = engine
        .load(
            Path::new(waos_test_programs::IMPORTS_INSPECT),
            &waos_test_programs::code_of(waos_test_programs::IMPORTS_INSPECT),
        )
        .unwrap();
    let grants = Arc::new(RoleGrants::default());
    let running = engine.run(&inspector, with(&grants)).unwrap();
    wait_until("the role asked about", || {
        !shell.prompts.roles_asked().is_empty()
    });
    let mut permissions = running
        .trace(TraceKind::Permissions)
        .expect("the access is made before the role is checked");
    shell.prompts.answer_roles(Some(Answer::Denied));
    let event = within(async move { permissions.next().await }).expect("the answer is told");
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(
        json["kind"]["question-answered"],
        serde_json::json!({"question": "role.inspector", "answer": "deny"})
    );
    within(running.ended());
}

#[test]
fn a_program_with_no_grants_holds_no_role() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let inspector = engine
        .load(
            Path::new(waos_test_programs::IMPORTS_INSPECT),
            &waos_test_programs::code_of(waos_test_programs::IMPORTS_INSPECT),
        )
        .unwrap();
    let running = engine
        .run(&inspector, ProgramEnvironment::default())
        .unwrap();
    let end = within(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
    assert!(
        shell.prompts.roles_asked().is_empty(),
        "nothing could keep the answer"
    );
}

#[test]
fn a_program_granted_its_role_sees_the_programs_and_traces_one() {
    let shell = TestShell::new();
    let config = WaOSConfig {
        inspected_programs: Some(Arc::new(TwoPrograms)),
        ..config()
    };
    let engine = WaOS::new(config, shell.shell(), shell.platform()).unwrap();
    let forever = engine
        .load(
            Path::new(waos_test_programs::FOREVER),
            &waos_test_programs::code_of(waos_test_programs::FOREVER),
        )
        .unwrap();
    let counted = Arc::new(RecordedOutput::default());
    let counting = engine.run(&forever, printing_to(&counted)).unwrap();
    let inspect = engine
        .load(
            Path::new(waos_test_programs::INSPECT),
            &waos_test_programs::code_of(waos_test_programs::INSPECT),
        )
        .unwrap();
    let grants = Arc::new(RoleGrants {
        inspector: Mutex::new(Some(Answer::Granted)),
        ..RoleGrants::default()
    });
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        ..printing_to(&output)
    };
    let inspecting = engine.run(&inspect, environment).unwrap();
    let end = within(inspecting.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    let said = output.lines().join("\n");
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
    within(counting.ended());
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

#[test]
fn a_program_importing_no_role_is_asked_about_none() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let incompatible = engine
        .load(
            Path::new(waos_test_programs::INCOMPATIBLE),
            &waos_test_programs::code_of(waos_test_programs::INCOMPATIBLE),
        )
        .unwrap();
    let running = engine
        .run(&incompatible, ProgramEnvironment::default())
        .unwrap();
    let end = within(running.ended());
    assert_eq!(not_granted(&end), None, "{end:?}");
    assert!(shell.prompts.roles_asked().is_empty());
}

#[test]
fn a_waiting_role_question_is_answered_by_setting_its_row_and_drops_its_prompt() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let inspector = engine
        .load(
            Path::new(waos_test_programs::IMPORTS_INSPECT),
            &waos_test_programs::code_of(waos_test_programs::IMPORTS_INSPECT),
        )
        .unwrap();
    let grants = Arc::new(RoleGrants::default());
    let running = engine.run(&inspector, with(&grants)).unwrap();
    wait_until("the role asked about", || {
        !shell.prompts.roles_asked().is_empty()
    });
    assert!(!shell.prompts.role_cancelled(0));

    grants.set(Answer::Denied);
    wait_until("the role prompt future is dropped", || {
        shell.prompts.role_cancelled(0)
    });
    let end = within(running.ended());
    assert_eq!(not_granted(&end), Some(Role::Inspector), "{end:?}");
}
