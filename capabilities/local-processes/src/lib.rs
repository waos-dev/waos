//! This computer as a machine a program acts on: the provider of `waos-capabilities`' [`Machines`]
//! for the machine in front of the user, which offers this computer alone. Its files are given from
//! `/`; its commands run with pipes (`pipes`) or in a terminal of their own (`terminal`), as the
//! user running WaOS, in the user's login environment read once (`login_environment`), each
//! command's variables on top (`starting`), its home the one the embedder gives, where it gives one
//! ([`HomeFolder`]). A started command is in a group of its own, which takes its signals, under the
//! names `system` gives each system's: a process group on Unix's (`process_group`), its terminal a
//! pseudo-terminal (`pseudo_terminal`); on Windows a job, its terminal a pseudoconsole (`windows`).
//! What it runs (`platform`) and its user's login shell, the user database's on Unix's systems
//! (`user_database`) and none on Windows, are read once, as it is made.
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
mod platform;
#[cfg(unix)]
mod process_group;
#[cfg(unix)]
mod pseudo_terminal;
mod starting;
mod system;
mod terminal;
#[cfg(unix)]
mod user_database;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use waos_capabilities::{
    Command, ExecError, InTerminal, Machine, MachineChoice, MachineFiles, Machines, Platform,
    TerminalSize, WithPipes,
};

pub use login_environment::{LoginEnvironment, Timer, Variables};

/// The user's home on this computer, read as each command starts, where the embedder knows it
/// better than the login environment: on Android, which gives an app no home of its own, the
/// phone's shared storage once the app may reach it.
pub type HomeFolder = Arc<dyn Fn() -> PathBuf + Send + Sync>;

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
        LocalMachines::of(login, None)
    }

    /// This computer, its commands run in `login` with `home` as the user's home.
    pub fn with_home(login: LoginEnvironment, home: HomeFolder) -> LocalMachines {
        LocalMachines::of(login, Some(home))
    }

    fn of(login: LoginEnvironment, home: Option<HomeFolder>) -> LocalMachines {
        LocalMachines {
            this_computer: Arc::new(ThisComputer {
                login: Arc::new(login),
                home,
                platform: platform::this_computer(),
                login_shell: login_shell(),
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
    /// The user's home, where the embedder gives it; the login environment's otherwise.
    home: Option<HomeFolder>,
    platform: Platform,
    login_shell: Option<String>,
}

/// The user's login shell: the user database's on Unix's systems; none on Windows, which has none.
fn login_shell() -> Option<String> {
    #[cfg(unix)]
    return user_database::login_shell();
    #[cfg(windows)]
    return None;
}

impl Machine for ThisComputer {
    fn name(&self) -> String {
        NAME.to_owned()
    }

    fn platform(&self) -> Platform {
        self.platform.clone()
    }

    fn login_shell(&self) -> Option<String> {
        self.login_shell.clone()
    }

    fn files(&self) -> MachineFiles {
        MachineFiles::Local(PathBuf::from("/"))
    }

    fn exec(&self, command: Command) -> BoxFuture<'static, Result<WithPipes, ExecError>> {
        let environment = self.environment();
        async move { pipes::exec(&command, &environment.await) }.boxed()
    }

    fn exec_in_terminal(
        &self,
        command: Command,
        size: TerminalSize,
    ) -> BoxFuture<'static, Result<InTerminal, ExecError>> {
        let environment = self.environment();
        async move { terminal::exec_in_terminal(&command, size, &environment.await) }.boxed()
    }
}

impl ThisComputer {
    /// The variables a command starts from: the login environment's, the user's home the one the
    /// embedder gives as the command starts, where it gives one.
    fn environment(&self) -> BoxFuture<'static, Variables> {
        let (login, home) = (Arc::clone(&self.login), self.home.clone());
        async move {
            let mut variables = login.variables().await.clone();
            if let Some(home) = home {
                starting::set_home(&mut variables, home());
            }
            variables
        }
        .boxed()
    }
}

#[cfg(all(test, unix))]
mod tests;
#[cfg(all(test, windows))]
mod tests_windows;
