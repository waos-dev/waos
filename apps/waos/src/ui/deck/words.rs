//! What the deck says.

use waos::{IncompatibleProgram, ProgramEnd, Role, RoleNotGranted};

/// What names the home, under its card.
pub(crate) const HOME: &str = "Home";
/// On the overview's toolbar, which goes back to the current tab's page.
pub(crate) const DONE: &str = "Done";

/// How many tabs are open, on the overview's toolbar.
pub(crate) fn tab_count(count: usize) -> String {
    if count == 1 {
        "1 tab".to_owned()
    } else {
        format!("{count} tabs")
    }
}

/// Under a program's name, on its page, while it starts.
pub(crate) const STARTING: &str = "Starting…";

/// How a program ended, as its tab says it: a plain line, and, for a failure, the engine's own
/// words beneath, for the user to read and report.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ending {
    pub(crate) said: String,
    pub(crate) detail: Option<String>,
}

/// What a tab says once `name` ended, as `end` says.
pub(crate) fn ended(name: &str, end: &ProgramEnd) -> Ending {
    let plain = |said: String| Ending { said, detail: None };
    match end {
        ProgramEnd::Returned(Ok(())) => plain(format!("{name} ended.")),
        ProgramEnd::Returned(Err(())) => plain(format!("{name} ended with a failure.")),
        ProgramEnd::Failed(error) => {
            if let Some(incompatible) = error.downcast_ref::<IncompatibleProgram>() {
                Ending {
                    said: format!(
                        "{name} was built for a different version of WaOS and cannot run in this one. Get an update of {name}, or update WaOS."
                    ),
                    detail: Some(incompatible.detail().to_owned()),
                }
            } else if let Some(not_granted) = error.downcast_ref::<RoleNotGranted>() {
                plain(role_not_granted(name, not_granted.role()))
            } else {
                Ending {
                    said: format!("{name} stopped because of a problem."),
                    detail: Some(format!("{error:#}")),
                }
            }
        }
        ProgramEnd::Stopped => plain(format!("{name} was stopped.")),
    }
}

/// What a tab says of `name`, which asks for `role` and was not granted it.
fn role_not_granted(name: &str, role: Role) -> String {
    let what = match role {
        Role::Inspector => "see what your other programs do",
    };
    format!(
        "{name} asks to {what}, and is not allowed to, so it did not start. You can change this in its settings in WaOS's library."
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use waos::ProgramEnvironment;
    use waos_test_programs::CODE_CACHE;

    use super::*;
    use crate::logic::test_engine::{self, NoWindows};

    fn said_alone(said: &str) -> Ending {
        Ending {
            said: said.to_owned(),
            detail: None,
        }
    }

    #[test]
    fn a_tab_says_how_its_program_ended() {
        assert_eq!(
            ended("Triangle", &ProgramEnd::Returned(Ok(()))),
            said_alone("Triangle ended.")
        );
        assert_eq!(
            ended("Triangle", &ProgramEnd::Returned(Err(()))),
            said_alone("Triangle ended with a failure.")
        );
        assert_eq!(
            ended("Triangle", &ProgramEnd::Stopped),
            said_alone("Triangle was stopped.")
        );
    }

    #[test]
    fn a_failure_is_said_plainly_over_the_engines_words() {
        let error = anyhow::anyhow!("wasm trap: unreachable").context("running the program");
        assert_eq!(
            ended("Trap", &ProgramEnd::Failed(Arc::new(error))),
            Ending {
                said: "Trap stopped because of a problem.".to_owned(),
                detail: Some("running the program: wasm trap: unreachable".to_owned()),
            }
        );
    }

    #[test]
    fn a_program_built_for_another_version_is_said_with_what_to_do_over_the_engines_words() {
        let ending = ended("Old", &incompatible_end());
        assert_eq!(
            ending.said,
            "Old was built for a different version of WaOS and cannot run in this one. Get an update of Old, or update WaOS."
        );
        let detail = ending.detail.expect("the engine's words");
        assert!(
            detail.starts_with("component imports instance `wasdk:platform/system@1.0.0`"),
            "wasmtime's own words, whole: {detail}"
        );
    }

    #[test]
    fn a_program_not_granted_its_role_is_said_with_where_to_change_it() {
        let ending = ended("Inspector", &end_of(waos_test_programs::IMPORTS_INSPECT));
        assert_eq!(
            ending,
            said_alone(
                "Inspector asks to see what your other programs do, and is not allowed to, so it did not start. You can change this in its settings in WaOS's library."
            )
        );
    }

    /// How the engine ends the component built for another version of the interface.
    fn incompatible_end() -> ProgramEnd {
        end_of(waos_test_programs::INCOMPATIBLE)
    }

    /// How the engine ends the component at `path`, run with nothing granted.
    fn end_of(path: &str) -> ProgramEnd {
        let engine = test_engine::engine(Arc::new(NoWindows), Path::new(CODE_CACHE));
        let program = engine.load(Path::new(path)).expect("it loads");
        let (running, _given) = engine
            .run(&program, ProgramEnvironment::default(), Vec::new())
            .expect("it starts");
        futures::executor::block_on(running.ended())
    }
}
