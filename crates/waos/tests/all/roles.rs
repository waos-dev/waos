//! A program's roles, read from what its component imports: one that imports `wasdk:inspect`
//! asks for the inspector role, is asked about at its start while its user has not answered,
//! the answer kept, and does not start without it, told apart as not granted.

use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use waos::{
    Answer, IncompatibleProgram, ProgramEnd, ProgramEnvironment, ProgramGrants, Question, Role,
    RoleNotGranted,
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
    let inspector = shell::load(&engine, waos_test_programs::INSPECTOR);
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
fn a_program_with_no_grants_holds_no_role() {
    let (engine, shell, _arrivals) = shell::engine();
    let inspector = shell::load(&engine, waos_test_programs::INSPECTOR);
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
fn a_program_granted_its_role_goes_on_to_be_linked() {
    let (engine, shell, _arrivals) = shell::engine();
    let inspector = shell::load(&engine, waos_test_programs::INSPECTOR);
    let grants = Arc::new(RoleGrants {
        inspector: Mutex::new(Some(Answer::Granted)),
    });
    let (running, _given) = engine.run(&inspector, with(&grants), Vec::new()).unwrap();
    let end = block_on(running.ended());
    // Nothing serves `wasdk:inspect` yet, so its link fails after the role is checked.
    let ProgramEnd::Failed(error) = &end else {
        panic!("nothing links it yet: {end:?}");
    };
    assert!(
        error.downcast_ref::<IncompatibleProgram>().is_some(),
        "{error:#}"
    );
    assert!(shell.roles_asked().is_empty());
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
