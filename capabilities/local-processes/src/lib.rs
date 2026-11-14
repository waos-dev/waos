//! This computer as a machine a program acts on (the processes proposal's decision 2): the
//! provider of `waos-capabilities`' [`Machines`] for the machine in front of the user, which
//! offers this computer alone. Its files are given from `/`; its commands run with pipes
//! (`pipes`) or in a terminal of their own (`terminal`), as the user running WaOS, in the user's
//! login environment read once (`login_environment`, decision 4), each command's variables on top
//! (`starting`). A started command is in a group of its own, which takes its signals, under the
//! names `system` gives each system's: a process group on Unix's (`process_group`), its terminal a
//! pseudo-terminal (`pseudo_terminal`); on Windows a job, its terminal a pseudoconsole
//! (`windows`).
//!
//! The invariant: this crate depends on `waos-capabilities` only of ours. It names no shell, no
//! engine and no program's internals; a program gets it when its environment is chosen, at start,
//! and acts on this computer only once its user picked it.

// `std`'s attribute list for a started process, through which a command with pipes is given its
// job as it is created on Windows.
#![cfg_attr(windows, feature(windows_process_extensions_raw_attribute))]

mod login_environment;
#[cfg(unix)]
mod login_shell;
mod pipes;
#[cfg(unix)]
mod process_group;
#[cfg(unix)]
mod pseudo_terminal;
mod starting;
mod system;
mod terminal;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use waos_capabilities::{
    Command, ExecError, InTerminal, Machine, MachineChoice, MachineFiles, Machines, TerminalSize,
    WithPipes,
};

pub use login_environment::{LoginEnvironment, Timer, Variables};

/// What names this computer among a program's grants.
pub const THIS_COMPUTER: &str = "this-computer";

/// This computer's name, as the user knows it.
const NAME: &str = "This computer";

/// This computer's machines: itself.
pub struct LocalMachines {
    this_computer: Arc<ThisComputer>,
}

impl LocalMachines {
    /// This computer, its commands run in `login`.
    pub fn new(login: LoginEnvironment) -> LocalMachines {
        LocalMachines {
            this_computer: Arc::new(ThisComputer {
                login: Arc::new(login),
            }),
        }
    }
}

impl Machines for LocalMachines {
    fn offered(&self) -> Vec<MachineChoice> {
        vec![MachineChoice {
            id: THIS_COMPUTER.to_owned(),
            name: NAME.to_owned(),
        }]
    }

    fn machine(&self, id: &str) -> Option<Arc<dyn Machine>> {
        (id == THIS_COMPUTER).then(|| Arc::clone(&self.this_computer) as Arc<dyn Machine>)
    }
}

/// This computer, as the user running WaOS.
struct ThisComputer {
    login: Arc<LoginEnvironment>,
}

impl Machine for ThisComputer {
    fn name(&self) -> String {
        NAME.to_owned()
    }

    fn files(&self) -> MachineFiles {
        MachineFiles::Local(PathBuf::from("/"))
    }

    fn exec(&self, command: Command) -> BoxFuture<'static, Result<WithPipes, ExecError>> {
        let login = Arc::clone(&self.login);
        async move { pipes::exec(&command, login.variables().await) }.boxed()
    }

    fn exec_in_terminal(
        &self,
        command: Command,
        size: TerminalSize,
    ) -> BoxFuture<'static, Result<InTerminal, ExecError>> {
        let login = Arc::clone(&self.login);
        async move { terminal::exec_in_terminal(&command, size, login.variables().await) }.boxed()
    }
}

#[cfg(all(test, unix))]
mod tests;
#[cfg(all(test, windows))]
mod tests_windows;
