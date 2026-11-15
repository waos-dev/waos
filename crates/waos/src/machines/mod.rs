//! `wasdk:platform/machines`: the machines a program acts on as their user (the processes
//! proposal's decisions 1 to 6), each the provider's in its environment ([`Machines`], one per
//! kind), which give its files from its root and run its commands.
//!
//! The engine keeps which machines the program holds: those its user picked for it, kept among
//! its grants by their ids and read at each `granted`; a machine handed to the program, granted or
//! picked, stays its until it ends, as an open file does, so a command run on it is told through
//! the program's access (`access`) and held while the program holds it, but never refused. A pick
//! is asked of the user through the shell's [`ShellMachinePicker`], which shows what the providers
//! offer, unless the user set the machine
//! the picker answers among the program's settings: then that machine is the pick, and nothing is
//! shown; one no provider offers now is no pick, as a cancel is, never another. The program never
//! sees the machines it was not given. The interface is served to a program by `host`.
//!
//! The invariant: a program reaches a machine only once its user picked it; what the machine runs
//! then runs as its user, outside the program's sandbox, which nothing here confines.

pub(crate) mod host;

use std::future::{Future, ready};
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use waos_capabilities::{
    ChildInput, Command, ExecError, Machine as ProvidedMachine, MachineChoice, Machines as Provider,
};

use crate::access::{Happened, Held, ProgramAccess, Use};
use crate::running::RunningProgram;
use crate::shell::ShellMachinePicker;

/// `machines.machines`: the program's machines, which its store holds.
pub struct Machines;

/// `machines.machine`: one machine the program holds.
pub type Machine = Arc<dyn ProvidedMachine>;

/// `machines.child`: a command the program started on a machine, and its run, held while the
/// program holds it.
pub struct Child {
    started: Box<dyn waos_capabilities::Child>,
    _run: Held,
}

/// `machines.pipes`: a command's input, and its output and errors until the program takes them.
pub struct Pipes {
    input: Box<dyn ChildInput>,
    output: Option<BoxStream<'static, Vec<u8>>>,
    errors: Option<BoxStream<'static, Vec<u8>>>,
}

/// `machines.terminal`: a command's terminal, and its output until the program takes it.
pub struct Terminal {
    terminal: Box<dyn waos_capabilities::Terminal>,
    output: Option<BoxStream<'static, Vec<u8>>>,
}

/// What a program holds of machines: the providers its environment gives it, what its user
/// picked for it, and the shell's picker.
pub(crate) struct ProgramMachines {
    providers: Vec<Arc<dyn Provider>>,
    /// Where the machines the user picked are kept, and each run told.
    access: ProgramAccess,
    picker: Option<Arc<dyn ShellMachinePicker>>,
    program: RunningProgram,
}

impl ProgramMachines {
    /// `program`'s machines, from `providers`, as the grants `access` reads keep them and `picker`
    /// asks the user.
    pub(crate) fn new(
        providers: Vec<Arc<dyn Provider>>,
        access: ProgramAccess,
        picker: Option<Arc<dyn ShellMachinePicker>>,
        program: RunningProgram,
    ) -> ProgramMachines {
        ProgramMachines {
            providers,
            access,
            picker,
            program,
        }
    }

    /// Whether the program may be given any machine.
    pub(crate) fn any_provided(&self) -> bool {
        !self.providers.is_empty()
    }

    /// The machines the user picked for the program, as its grants keep them now, each a
    /// provider still offers.
    pub(crate) fn granted(&self) -> Vec<Machine> {
        let providers = self.providers.clone();
        self.access
            .grants()
            .machines()
            .iter()
            .filter_map(|id| machine_of(&providers, id))
            .collect()
    }

    /// Asks the user to pick one of the machines the providers offer, or takes the one the user
    /// set as the answer, and resolves to it, kept among the program's grants; to none when the
    /// user cancels, nobody can ask, or no provider offers the one set.
    pub(crate) fn pick(&self) -> impl Future<Output = Option<Machine>> + Send + 'static + use<> {
        let picked: BoxFuture<'static, Option<String>> =
            match (self.access.grants().machine_answer(), &self.picker) {
                (Some(set), _) => ready(Some(set)).boxed(),
                (None, Some(picker)) => picker.pick(&self.program, self.offered()),
                (None, None) => ready(None).boxed(),
            };
        let (providers, access) = (self.providers.clone(), self.access.clone());
        let program = self.program.name().to_owned();
        async move {
            let id = picked.await?;
            let Some(machine) = machine_of(&providers, &id) else {
                tracing::warn!(
                    program,
                    machine = id,
                    "no provider offers the machine picked"
                );
                return None;
            };
            access.tell(Happened::MachinePicked { machine: &id });
            access.grants().keep_machine(&id);
            Some(machine)
        }
    }

    /// The run of `command` on `machine`, in a terminal where `in_terminal`, told and held from
    /// now on, for the [`Child`] it starts to hold.
    pub(crate) fn hold_run(
        &self,
        machine: &Machine,
        command: &Command,
        in_terminal: bool,
    ) -> Result<Held, ExecError> {
        hold_run(&self.access, &machine.name(), command, in_terminal)
    }

    /// What every provider offers, in the providers' order.
    fn offered(&self) -> Vec<MachineChoice> {
        self.providers
            .iter()
            .flat_map(|provider| provider.offered())
            .collect()
    }
}

/// The run of `command` on the machine named `machine`, held: a machine handed to the program is
/// its whole, so no grant refuses what it runs.
fn hold_run(
    access: &ProgramAccess,
    machine: &str,
    command: &Command,
    in_terminal: bool,
) -> Result<Held, ExecError> {
    let used = Use::Run {
        machine,
        command,
        in_terminal,
    };
    match access.hold(used, |_grants| Ok(())) {
        Ok(((), run)) => Ok(run),
        Err(refusal) => Err(ExecError::Refused(format!(
            "refused by the program's setting {}",
            refusal.setting
        ))),
    }
}

impl Child {
    pub(crate) fn new(started: Box<dyn waos_capabilities::Child>, run: Held) -> Child {
        Child { started, _run: run }
    }

    pub(crate) fn started(&self) -> &dyn waos_capabilities::Child {
        self.started.as_ref()
    }
}

/// The machine `id` names, among what `providers` offer.
fn machine_of(providers: &[Arc<dyn Provider>], id: &str) -> Option<Machine> {
    providers.iter().find_map(|provider| provider.machine(id))
}

impl Pipes {
    fn new(
        input: Box<dyn ChildInput>,
        output: BoxStream<'static, Vec<u8>>,
        errors: BoxStream<'static, Vec<u8>>,
    ) -> Pipes {
        Pipes {
            input,
            output: Some(output),
            errors: Some(errors),
        }
    }
}

impl Terminal {
    fn new(
        terminal: Box<dyn waos_capabilities::Terminal>,
        output: BoxStream<'static, Vec<u8>>,
    ) -> Terminal {
        Terminal {
            terminal,
            output: Some(output),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Told;
    use crate::permissions::NothingGranted;

    #[test]
    fn a_run_is_told_allowed_whatever_is_granted_and_ends_as_its_hold_is_let_go() {
        let access = ProgramAccess::new(Arc::new(NothingGranted));
        let run = hold_run(&access, "This Mac", &Command::new("git"), false).unwrap();
        assert!(
            access.refusals().is_empty(),
            "a machine given is given whole"
        );
        assert!(matches!(access.told()[..], [Told::Allowed(_)]));
        drop(run);
        assert_eq!(access.told().last(), Some(&Told::Ended));
    }
}
