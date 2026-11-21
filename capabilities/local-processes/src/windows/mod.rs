//! A command on Windows, which has no process groups and no signals: its processes in a job object
//! of its own (`job`), given to the command as it is created, so every process it starts is in it
//! too; its terminal a pseudoconsole WaOS starts the command on itself (`pseudoconsole`), since the
//! job must be given at creation; and the user's environment as Windows builds it for a new logon
//! (`user_environment`), Windows having no login shell; and the processor Windows runs on
//! (`native_architecture`). A signal is what Windows has in its place:
//! an interrupt is `^C` typed into the command's pseudoconsole, which the console turns into the
//! console's Ctrl-C, and ends the job of a command run with pipes, which has no console; a hang-up
//! closes the pseudoconsole, which closes the command's console; anything else ends the job. A
//! command ended by Ctrl-C (`STATUS_CONTROL_C_EXIT`) ends by the signal `INT`, as a POSIX shell on
//! Windows reports it. A process started ignoring Ctrl-C, as one started in a process group of its
//! own is, passes that on to the commands it starts, whose interrupt is then lost: `waos`
//! starts WaOS in none.
//!
//! The invariant: wrapping Windows' calls is what this module is for, and the one place in the
//! crate where `unsafe` is written for them; every handle it opens is owned, and closed once.

// Windows' process, console, job and system calls are reached through `unsafe`; this module and
// its children write it, as `user_database` does for Unix's user database.
#![allow(unsafe_code)]

mod command_line;
mod job;
mod native_architecture;
mod pseudoconsole;
mod user_environment;

pub(crate) use job::{CommandJob, start_with_pipes};
pub(crate) use native_architecture::native_architecture;
pub(crate) use pseudoconsole::{TerminalSession, start_in_terminal};
pub(crate) use user_environment::user_environment;
