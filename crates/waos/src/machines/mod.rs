//! `wasdk:platform/machines`: the machines a program acts on as their user (the processes
//! proposal's decisions 1 to 6), each the provider's in its environment ([`Machines`], one per
//! kind), which give its files from its root and run its commands.
//!
//! The engine keeps which machines the program holds: those its user picked for it, kept among
//! its grants by their ids and read at each `granted`; a machine handed to the program, granted or
//! picked, stays its until it ends, as an open file does. A pick is asked of the user through the shell's
//! [`ShellMachinePicker`], which shows what the providers offer, unless the user set the machine
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
    ChildInput, Machine as ProvidedMachine, MachineChoice, Machines as Provider, ProgramGrants,
};

use crate::running::RunningProgram;
use crate::shell::ShellMachinePicker;

/// `machines.machines`: the program's machines, which its store holds.
pub struct Machines;

/// `machines.machine`: one machine the program holds.
pub type Machine = Arc<dyn ProvidedMachine>;

/// `machines.child`: a command the program started on a machine.
pub type Child = Box<dyn waos_capabilities::Child>;

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
    grants: Arc<dyn ProgramGrants>,
    picker: Option<Arc<dyn ShellMachinePicker>>,
    program: RunningProgram,
}

impl ProgramMachines {
    /// `program`'s machines, from `providers`, as `grants` keep them and `picker` asks the user.
    pub(crate) fn new(
        providers: Vec<Arc<dyn Provider>>,
        grants: Arc<dyn ProgramGrants>,
        picker: Option<Arc<dyn ShellMachinePicker>>,
        program: RunningProgram,
    ) -> ProgramMachines {
        ProgramMachines {
            providers,
            grants,
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
        self.grants
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
            match (self.grants.machine_answer(), &self.picker) {
                (Some(set), _) => ready(Some(set)).boxed(),
                (None, Some(picker)) => picker.pick(&self.program, self.offered()),
                (None, None) => ready(None).boxed(),
            };
        let (providers, grants) = (self.providers.clone(), Arc::clone(&self.grants));
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
            grants.keep_machine(&id);
            Some(machine)
        }
    }

    /// What every provider offers, in the providers' order.
    fn offered(&self) -> Vec<MachineChoice> {
        self.providers
            .iter()
            .flat_map(|provider| provider.offered())
            .collect()
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
