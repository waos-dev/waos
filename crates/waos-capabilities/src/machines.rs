//! The machines a program may act on as their user: this computer, and SSH machines once their
//! provider exists (the processes proposal's decision 2). A machine gives its files from its root
//! and runs commands, with pipes or in a terminal, as one SSH login gives files, `exec` and a
//! `pty`; and says what it runs and which shell its user logs in to, learnt once: on this computer
//! from the system, on an SSH machine by asking `uname -sm` and the shell SSH starts as it connects
//! ([`Platform::of_uname`] reads the first).
//!
//! A provider serves one kind of machine ([`Machines`]): it offers the user its machines to pick
//! from, and answers the machine an id names. Which machines a program holds is the user's, kept
//! among its grants by their ids; a provider asks nothing.
//!
//! The invariant: what a machine starts runs as the machine's user, outside the program's
//! sandbox (decision 1), in that user's login environment with the command's variables on top
//! (decision 4). Nothing here confines it.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::BoxStream;

/// The machines of one kind: what the user picks from, and the machine an id names. Called from
/// the program's thread.
pub trait Machines: Send + Sync {
    /// The machines the user may pick from, in the order a picker shows them.
    fn offered(&self) -> Vec<MachineChoice>;

    /// The machine `id` names, one this provider offers; none for an id it does not know.
    fn machine(&self, id: &str) -> Option<Arc<dyn Machine>>;
}

/// A machine as the user picks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineChoice {
    /// What names it among a program's grants: `this-computer`, or an SSH machine's address.
    pub id: String,
    /// Its name, as the user knows it.
    pub name: String,
}

/// One machine: its files, and the commands it runs. Called from the program's thread.
pub trait Machine: Send + Sync {
    /// Its name, as the user knows it.
    fn name(&self) -> String;

    /// What it runs.
    fn platform(&self) -> Platform;

    /// The shell its user logs in to, the one SSH starts; none where it has none, as on Windows.
    fn login_shell(&self) -> Option<String>;

    /// Where its files are, which the program is given from their root.
    fn files(&self) -> MachineFiles;

    /// Runs `command`, its input, output and errors each a pipe of its own; resolves once it has
    /// started, or to why it did not.
    fn exec(&self, command: Command) -> BoxFuture<'static, Result<WithPipes, ExecError>>;

    /// Runs `command` in a new terminal of `size`, which is its input, output and errors
    /// together; resolves once it has started, or to why it did not.
    fn exec_in_terminal(
        &self,
        command: Command,
        size: TerminalSize,
    ) -> BoxFuture<'static, Result<InTerminal, ExecError>>;
}

/// What a machine runs: its operating system and its processor's architecture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: OperatingSystem,
    pub arch: Architecture,
}

impl Platform {
    /// What `uname` names a system and an architecture, `uname -s` and `uname -m`, as Unix's
    /// systems answer them and an SSH machine's shell prints them.
    pub fn of_uname(system: &str, machine: &str) -> Platform {
        Platform {
            os: OperatingSystem::of_uname(system),
            arch: Architecture::of_uname(machine),
        }
    }
}

/// An operating system, as a machine names which it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperatingSystem {
    Macos,
    Linux,
    Windows,
    /// Another, by the name `uname -s` gives it.
    Other(String),
}

impl OperatingSystem {
    /// The system `uname -s` names: `Darwin` is macOS, and Android's kernel is Linux.
    fn of_uname(system: &str) -> OperatingSystem {
        match system {
            "Darwin" => OperatingSystem::Macos,
            "Linux" => OperatingSystem::Linux,
            other => OperatingSystem::Other(other.to_owned()),
        }
    }
}

/// A processor's architecture, as a machine names which it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Architecture {
    Aarch64,
    X86_64,
    /// Another, by the name `uname -m` gives it.
    Other(String),
}

impl Architecture {
    /// The architecture `uname -m` names: `arm64` on macOS and `aarch64` on Linux, `x86_64`, and
    /// the BSDs' `amd64`.
    fn of_uname(machine: &str) -> Architecture {
        match machine {
            "arm64" | "aarch64" => Architecture::Aarch64,
            "x86_64" | "amd64" => Architecture::X86_64,
            other => Architecture::Other(other.to_owned()),
        }
    }
}

/// Where a machine's files are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineFiles {
    /// On this computer, from this folder down: `/` for this computer itself. An SSH machine's
    /// files, over SFTP, come with its provider.
    Local(PathBuf),
}

/// What to run: a program, by its name on the machine's `PATH` or by its path, its arguments,
/// each passed as it is, where it starts, and its variables, on top of the machine user's login
/// environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub arguments: Vec<String>,
    /// The folder it starts in; the user's home folder when none is given.
    pub working_directory: Option<String>,
    /// Each variable, by its name, in the order set; a later one of a name replaces an earlier.
    pub variables: Vec<(String, String)>,
}

impl Command {
    /// `program`, with no arguments, starting in the user's home folder.
    pub fn new(program: impl Into<String>) -> Command {
        Command {
            program: program.into(),
            ..Command::default()
        }
    }
}

/// A command started with pipes.
pub struct WithPipes {
    pub child: Box<dyn Child>,
    /// Its input.
    pub input: Box<dyn ChildInput>,
    /// What it writes to its output, in chunks, until it closes it. While nobody reads it the
    /// command waits, as one whose pipe nobody reads does; dropping it throws away what follows.
    pub output: BoxStream<'static, Vec<u8>>,
    /// What it writes to its errors, as `output` is.
    pub errors: BoxStream<'static, Vec<u8>>,
}

/// A command started in a terminal of its own.
pub struct InTerminal {
    pub child: Box<dyn Child>,
    pub terminal: Box<dyn Terminal>,
    /// Everything the command writes to its terminal, in chunks, until the terminal closes, as
    /// [`WithPipes::output`] is.
    pub output: BoxStream<'static, Vec<u8>>,
}

/// A started command. Dropping it leaves the command running, until [`end`](Child::end) is
/// called, as the end of the program that started it calls it.
pub trait Child: Send + Sync {
    /// Sends `signal` to the command's process group, where the machine takes signals.
    fn signal(&self, signal: Signal);

    /// Ends the command, everything it started with it, as the end of the program that started it
    /// does: asked to end as a closed terminal or a stop asks (a hangup for one in a terminal, a
    /// termination for one with pipes), then forced after a short grace where it has not ended;
    /// nothing once it has ended. Returns at once.
    fn end(&self);

    /// How the command ended, once it has. Every call answers the same.
    fn exit_status(&self) -> BoxFuture<'static, ExitStatus>;
}

/// A started command's input pipe.
pub trait ChildInput: Send + Sync {
    /// Writes `data` to the command's input; resolves once the pipe has taken it.
    fn write(&self, data: Vec<u8>) -> BoxFuture<'static, Result<(), ExecError>>;

    /// Closes the command's input, which it reads to its end. Dropping it closes it too.
    fn close(&self);
}

/// A started command's terminal. Dropping it closes the terminal, which hangs the command up,
/// as closing a terminal window does.
pub trait Terminal: Send + Sync {
    /// Writes `data` to the terminal, as typed; resolves once the terminal has taken it.
    fn write(&self, data: Vec<u8>) -> BoxFuture<'static, Result<(), ExecError>>;

    /// Tells the command the terminal's new size.
    fn resize(&self, size: TerminalSize);
}

/// A terminal's size in character cells, and in pixels where they are known, else 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
    pub width_in_pixels: u16,
    pub height_in_pixels: u16,
}

/// A signal for a started command and every process of its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// `SIGHUP`, as if its terminal went away.
    Hangup,
    /// `SIGINT`, what Ctrl-C sends, for a command with no terminal to send it through: a console
    /// process group's `CTRL_C_EVENT` on Windows, SSH's `INT`.
    Interrupt,
    /// `SIGTERM`: asked to end.
    Terminate,
    /// `SIGKILL`: ended at once.
    Kill,
}

/// How a started command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    /// It exited with this code.
    Exited(u32),
    /// A signal ended it: the signal's name without `SIG`, as SSH names it, such as `TERM`.
    Signaled(String),
    /// The machine never said.
    Unknown,
}

/// Why a command was not run, or its input not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// The machine would not run it: no such program, or not allowed.
    Refused(String),
    /// The machine could not be reached, or failed while running it.
    Failed(String),
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::Refused(why) => write!(f, "refused: {why}"),
            ExecError::Failed(why) => write!(f, "failed: {why}"),
        }
    }
}

impl std::error::Error for ExecError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uname_s_names_are_read_as_the_systems_and_architectures_they_are() {
        assert_eq!(
            Platform::of_uname("Darwin", "arm64"),
            Platform {
                os: OperatingSystem::Macos,
                arch: Architecture::Aarch64,
            }
        );
        assert_eq!(
            Platform::of_uname("Linux", "x86_64"),
            Platform {
                os: OperatingSystem::Linux,
                arch: Architecture::X86_64,
            }
        );
        assert_eq!(
            Platform::of_uname("FreeBSD", "riscv64"),
            Platform {
                os: OperatingSystem::Other("FreeBSD".to_owned()),
                arch: Architecture::Other("riscv64".to_owned()),
            }
        );
    }
}
