//! A program's roles: what it may be besides a program, asked for by importing a role's
//! interfaces (decision 9 of `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`),
//! and granted by its user by name. Before a program is linked, the roles its component asks for
//! are read from its imports, and each is checked against the user's answer, asked at this start
//! when there is none: a program the user did not grant a role it asks for does not start, and is
//! told so rather than wasmtime's missing import.
//!
//! The invariant: a role is read from what the component imports and nothing else, for an
//! installed package and a development run alike; nothing the program declares, and no signature,
//! grants one.

use std::error::Error;
use std::fmt;

use wasmtime::Engine;
use wasmtime::component::Component;

use crate::access::{ProgramAccess, Prompt};
use crate::running::RunningProgram;
use crate::shell::Prompts;
use waos_capabilities::{Answer, Role};

/// The WIT package whose interfaces a program imports to ask for `role`.
fn package_of(role: Role) -> &'static str {
    match role {
        Role::Inspector => "wasdk:inspect",
    }
}

/// The roles `component` asks for: those whose package's interfaces it imports.
pub(crate) fn asked_for(component: &Component, engine: &Engine) -> Vec<Role> {
    let imports: Vec<String> = component
        .component_type()
        .imports(engine)
        .map(|(name, _)| name.to_owned())
        .collect();
    Role::ALL
        .into_iter()
        .filter(|role| {
            imports
                .iter()
                .any(|name| imports_from(name, package_of(*role)))
        })
        .collect()
}

/// Whether the import `name`, `wasdk:inspect/devices@0.1.0`, is an interface of `package`.
fn imports_from(name: &str, package: &str) -> bool {
    name.strip_prefix(package)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Checks that `program`'s user grants it each of `asked`, by the grants of its `access`, asking
/// at once for a role it has no answer to, told as a question put through the access, and keeping
/// the answer; the first role not granted refuses the start. A program given no grants has
/// nothing granted.
pub(crate) async fn check(
    asked: &[Role],
    access: &ProgramAccess,
    prompts: &dyn Prompts,
    program: &RunningProgram,
) -> Result<(), RoleNotGranted> {
    for role in asked.iter().copied() {
        let not_granted = RoleNotGranted { role };
        let answer = match access.grants().role(role) {
            Some(answer) => answer,
            None => {
                let asked = prompts.ask_for_role(program, role);
                let answer = access
                    .put_question(Prompt::Role(role), asked)
                    .await
                    .ok_or(not_granted)?;
                access.grants().set_role(role, answer);
                answer
            }
        };
        if answer != Answer::Granted {
            return Err(not_granted);
        }
    }
    Ok(())
}

/// The program asks for a role, by importing its interfaces, that its user did not grant it, so
/// nothing of it ran. What a failed [`ProgramEnd`](crate::ProgramEnd) carries in its error's
/// chain, where `downcast_ref` finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleNotGranted {
    role: Role,
}

impl RoleNotGranted {
    /// The role not granted.
    pub fn role(&self) -> Role {
        self.role
    }
}

impl fmt::Display for RoleNotGranted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (package, name) = match self.role {
            Role::Inspector => (package_of(self.role), "inspector"),
        };
        write!(
            formatter,
            "the program imports {package} and was not granted the {name} role"
        )
    }
}

impl Error for RoleNotGranted {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_import_is_a_roles_where_it_is_an_interface_of_the_roles_package() {
        assert!(imports_from("wasdk:inspect/devices@0.1.0", "wasdk:inspect"));
        assert!(imports_from("wasdk:inspect/trace", "wasdk:inspect"));
        assert!(!imports_from(
            "wasdk:inspector/devices@0.1.0",
            "wasdk:inspect"
        ));
        assert!(!imports_from("wasdk:platform/views@0.1.0", "wasdk:inspect"));
    }
}
