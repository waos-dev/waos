//! A started command as this computer knows it: the leader of a process group of its own, which
//! takes the signals sent to the command, and how it ended, by the names SSH gives signals.
//!
//! The invariant: a signal goes to the command's group only until its end has been collected, since
//! from then on its id may name another's.

use std::io;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::Command as Process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use rustix::process::{Pid, Signal as OsSignal};
use waos_capabilities::{Child, ExitStatus, Signal};

use crate::pipes::StartedWithPipes;

/// The signals SSH names (RFC 4254, §6.10) and the few others a command commonly ends by, with
/// the names an exit status answers them by.
const SIGNAL_NAMES: [(OsSignal, &str); 16] = [
    (OsSignal::ABORT, "ABRT"),
    (OsSignal::ALARM, "ALRM"),
    (OsSignal::FPE, "FPE"),
    (OsSignal::HUP, "HUP"),
    (OsSignal::ILL, "ILL"),
    (OsSignal::INT, "INT"),
    (OsSignal::KILL, "KILL"),
    (OsSignal::PIPE, "PIPE"),
    (OsSignal::QUIT, "QUIT"),
    (OsSignal::SEGV, "SEGV"),
    (OsSignal::TERM, "TERM"),
    (OsSignal::USR1, "USR1"),
    (OsSignal::USR2, "USR2"),
    (OsSignal::BUS, "BUS"),
    (OsSignal::SYS, "SYS"),
    (OsSignal::TRAP, "TRAP"),
];

/// Starts `process`, its three pipes piped, leading a process group of its own, on tokio's
/// runtime.
pub(crate) fn start_with_pipes(mut process: Process) -> io::Result<StartedWithPipes> {
    process.process_group(0);
    let mut child = tokio::process::Command::from(process).spawn()?;
    let leader = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("the command has no process id"))?;
    let (group, collector) = ProcessGroup::led_by(leader);
    let (input, output, errors) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let end = async move {
        match child.wait().await {
            Ok(status) => exit_status_of(status.code(), status.signal()),
            Err(_) => ExitStatus::Unknown,
        }
    };
    Ok(StartedWithPipes {
        input: input.expect("a piped input"),
        output: output.expect("a piped output"),
        errors: errors.expect("piped errors"),
        group,
        collector,
        end: end.boxed(),
    })
}

/// A started command leading its own process group, and how it ended once that is collected:
/// one for its child and one for its terminal, each a copy.
#[derive(Clone)]
pub(crate) struct ProcessGroup {
    leader: Pid,
    /// Whether the command's end has been collected, after which its id is no longer its own.
    collected: Arc<AtomicBool>,
    exit: Shared<oneshot::Receiver<ExitStatus>>,
}

/// Where whoever collects a command's end says how it ended.
pub(crate) struct Collector {
    collected: Arc<AtomicBool>,
    ended: oneshot::Sender<ExitStatus>,
}

impl ProcessGroup {
    /// The group `leader` leads, and what the one that collects its end tells it through.
    pub(crate) fn led_by(leader: Pid) -> (ProcessGroup, Collector) {
        let collected = Arc::new(AtomicBool::new(false));
        let (ended, exit) = oneshot::channel();
        let group = ProcessGroup {
            leader,
            collected: Arc::clone(&collected),
            exit: exit.shared(),
        };
        (group, Collector { collected, ended })
    }

    /// Sends `signal` to the group while its leader's id is still its own.
    pub(crate) fn send(&self, signal: OsSignal) {
        if self.collected.load(Ordering::Acquire) {
            return;
        }
        // A group that has gone, or refuses the signal, is a signal the machine did not deliver.
        let _ = rustix::process::kill_process_group(self.leader, signal);
    }
}

impl Child for ProcessGroup {
    fn signal(&self, signal: Signal) {
        self.send(os_signal_of(signal));
    }

    fn exit_status(&self) -> BoxFuture<'static, ExitStatus> {
        self.exit
            .clone()
            .map(|ended| ended.unwrap_or(ExitStatus::Unknown))
            .boxed()
    }
}

impl Collector {
    /// The command ended as `status` says, and its end is collected.
    pub(crate) fn collected(self, status: ExitStatus) {
        self.collected.store(true, Ordering::Release);
        // Nobody listening is nobody to tell.
        let _ = self.ended.send(status);
    }
}

/// How a command ended: the code it exited with, or the signal that ended it.
pub(crate) fn exit_status_of(code: Option<i32>, signal: Option<i32>) -> ExitStatus {
    if let Some(code) = code {
        return u32::try_from(code).map_or(ExitStatus::Unknown, ExitStatus::Exited);
    }
    match signal {
        Some(signal) => ExitStatus::Signaled(name_of_signal(signal)),
        None => ExitStatus::Unknown,
    }
}

/// The signal numbered `number` by its name without `SIG`, or by its number where it has no
/// name here.
fn name_of_signal(number: i32) -> String {
    SIGNAL_NAMES
        .iter()
        .find(|(signal, _)| signal.as_raw() == number)
        .map_or_else(|| number.to_string(), |(_, name)| (*name).to_owned())
}

fn os_signal_of(signal: Signal) -> OsSignal {
    match signal {
        Signal::Hangup => OsSignal::HUP,
        Signal::Interrupt => OsSignal::INT,
        Signal::Terminate => OsSignal::TERM,
        Signal::Kill => OsSignal::KILL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_end_is_its_code_or_the_signal_s_name() {
        assert_eq!(exit_status_of(Some(3), None), ExitStatus::Exited(3));
        let term = OsSignal::TERM.as_raw();
        assert_eq!(
            exit_status_of(None, Some(term)),
            ExitStatus::Signaled("TERM".to_owned())
        );
        assert_eq!(
            exit_status_of(None, Some(200)),
            ExitStatus::Signaled("200".to_owned())
        );
        assert_eq!(exit_status_of(None, None), ExitStatus::Unknown);
    }
}
