//! A command's terminal on Unix's systems: a pseudo-terminal through portable-pty, which makes the
//! command the leader of a session whose controlling terminal it is, its group taking the signals
//! sent to it and the hang-up when the terminal closes. Ported from the experiment's `wasdk-io`
//! (`local/terminals.rs`).
//!
//! The invariant: hanging up signals the command's group only while its id is its own.

use std::sync::{Mutex, PoisonError};

use portable_pty::{CommandBuilder, MasterPty, PtySize, SlavePty, native_pty_system};
use rustix::process::{Pid, Signal as OsSignal, WaitOptions};
use waos_capabilities::{ExecError, ExitStatus, TerminalSize};

use crate::process_group::{Collector, ProcessGroup, exit_status_of};
use crate::starting::Starting;
use crate::terminal::{StartedTerminal, spawn_thread};

/// This end of a command's pseudo-terminal, which sets its size, and the command's group, which
/// its closing hangs up.
pub(crate) struct TerminalSession {
    terminal: Mutex<Box<dyn MasterPty + Send>>,
    group: ProcessGroup,
}

impl TerminalSession {
    /// Tells the command the terminal's new size.
    pub(crate) fn resize(&self, size: TerminalSize) {
        let terminal = self.terminal.lock().unwrap_or_else(PoisonError::into_inner);
        // A terminal that has closed has no size left to set.
        let _ = terminal.resize(pty_size_of(size));
    }

    /// Hangs the command up, as the terminal closes.
    pub(crate) fn hang_up(&self) {
        self.group.send(OsSignal::HUP);
    }
}

/// Starts `starting` in a new pseudo-terminal of `size`.
pub(crate) fn start_in_terminal(
    starting: Starting,
    size: TerminalSize,
) -> Result<StartedTerminal, ExecError> {
    let terminal = native_pty_system()
        .openpty(pty_size_of(size))
        .map_err(failure_of)?;
    let reader = terminal.master.try_clone_reader().map_err(failure_of)?;
    let writer = terminal.master.take_writer().map_err(failure_of)?;
    let leader = start_on(terminal.slave, starting)?;
    let (group, collector) = ProcessGroup::led_by(leader, OsSignal::HUP);
    wait_on_a_thread(leader, collector);
    Ok(StartedTerminal {
        child: Box::new(group.clone()),
        session: TerminalSession {
            terminal: Mutex::new(terminal.master),
            group,
        },
        reader,
        writer,
    })
}

/// Starts `starting` on the terminal's end `started_end`, which only the command holds from then
/// on: kept here, it would keep the terminal open after the command ended.
fn start_on(started_end: Box<dyn SlavePty + Send>, starting: Starting) -> Result<Pid, ExecError> {
    let program = starting.program.clone();
    let mut command_line = CommandBuilder::new(&starting.program);
    command_line.args(&starting.arguments);
    command_line.env_clear();
    for (name, value) in &starting.variables {
        command_line.env(name, value);
    }
    command_line.cwd(&starting.folder);
    let started = started_end
        .spawn_command(command_line)
        .map_err(|error| ExecError::Refused(format!("{program}: {error:#}")))?;
    started
        .process_id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .ok_or_else(|| ExecError::Failed("the command has no process id".to_owned()))
}

/// Waits for `leader` to end on a thread of its own, and tells `collector` how.
fn wait_on_a_thread(leader: Pid, collector: Collector) {
    spawn_thread("terminal exit", move || {
        collector.collected(wait_for_end_of(leader));
    });
}

/// Waits for `leader` to end, and collects how it did.
fn wait_for_end_of(leader: Pid) -> ExitStatus {
    loop {
        match rustix::process::waitpid(Some(leader), WaitOptions::empty()) {
            Ok(Some((_, status))) => {
                return exit_status_of(status.exit_status(), status.terminating_signal());
            }
            Err(rustix::io::Errno::INTR) => {}
            Ok(None) | Err(_) => return ExitStatus::Unknown,
        }
    }
}

fn pty_size_of(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.columns,
        pixel_width: size.width_in_pixels,
        pixel_height: size.height_in_pixels,
    }
}

fn failure_of(error: anyhow::Error) -> ExecError {
    ExecError::Failed(format!("{error:#}"))
}
