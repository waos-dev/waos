//! A started command as this computer knows it: the leader of a process group of its own, which
//! takes the signals sent to the command, and how it ended, by the names SSH gives signals.
//!
//! A command ends with the program that started it ([`Child::end`]): its group is asked to end the
//! way a closed terminal or a stop asks, a hangup for a command in a terminal and a termination for
//! one with pipes, then killed after `GRACE` where its leader has not ended by then.
//!
//! The invariant: a signal goes to the command's group only until its end has been collected, since
//! from then on its id may name another's.

use std::io;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::Command as Process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use rustix::process::{Pid, Signal as OsSignal};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use waos_capabilities::{Child, ExitStatus, Signal};

use crate::pipes::StartedWithPipes;

/// How long a command ended with its program has to end its own way before it is killed.
const GRACE: Duration = Duration::from_secs(2);

/// How often a command being ended is looked at, to know whether it has.
const ENDING_CHECK: Duration = Duration::from_millis(20);

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

/// Starts `process`, its three pipes piped, leading a process group of its own: its pipes read and
/// written on tokio's runtime, and its end waited for and collected on a thread of its own, as
/// `std` starts it, not tokio, so the end is heard after the runtime of the program that ran it
/// has gone.
pub(crate) fn start_with_pipes(mut process: Process) -> io::Result<StartedWithPipes> {
    process.process_group(0);
    let mut child = process.spawn()?;
    let leader = i32::try_from(child.id())
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("the command has no process id"))?;
    let (group, collector) = ProcessGroup::led_by(leader, OsSignal::TERM);
    let (input, output, errors) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    collect_on_a_thread(child, collector);
    Ok(StartedWithPipes {
        input: ChildStdin::from_std(input.expect("a piped input"))?,
        output: ChildStdout::from_std(output.expect("a piped output"))?,
        errors: ChildStderr::from_std(errors.expect("piped errors"))?,
        group,
    })
}

/// Waits for `child` to end on a thread of its own, and collects how it did; where no thread
/// starts, its end is told as unknown.
fn collect_on_a_thread(mut child: std::process::Child, collector: Collector) {
    let waiting = std::thread::Builder::new()
        .name("command exit".to_owned())
        .spawn(move || {
            let status = match child.wait() {
                Ok(status) => exit_status_of(status.code(), status.signal()),
                Err(_) => ExitStatus::Unknown,
            };
            collector.collected(status);
        });
    if let Err(error) = waiting {
        tracing::warn!(%error, "no thread waits for a command's end");
    }
}

/// A started command leading its own process group, and how it ended once that is collected:
/// one for its child and one for its terminal, each a copy.
#[derive(Clone)]
pub(crate) struct ProcessGroup {
    leader: Pid,
    /// What asks the command to end its own way, as its program ends.
    ending: OsSignal,
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
    /// The group `leader` leads, asked to end by `ending`, and what the one that collects its end
    /// tells it through.
    pub(crate) fn led_by(leader: Pid, ending: OsSignal) -> (ProcessGroup, Collector) {
        let collected = Arc::new(AtomicBool::new(false));
        let (ended, exit) = oneshot::channel();
        let group = ProcessGroup {
            leader,
            ending,
            collected: Arc::clone(&collected),
            exit: exit.shared(),
        };
        (group, Collector { collected, ended })
    }

    /// Sends `signal` to the group while its leader's id is still its own.
    pub(crate) fn send(&self, signal: OsSignal) {
        if self.is_collected() {
            return;
        }
        // A group that has gone, or refuses the signal, is a signal the machine did not deliver.
        let _ = rustix::process::kill_process_group(self.leader, signal);
    }

    fn is_collected(&self) -> bool {
        self.collected.load(Ordering::Acquire)
    }

    /// Kills the group once `GRACE` has passed, unless its leader ended before, looked at on a
    /// thread of its own.
    fn kill_after_grace(&self) {
        let group = self.clone();
        let killing = thread::Builder::new()
            .name("command ending".to_owned())
            .spawn(move || {
                let deadline = Instant::now() + GRACE;
                while !group.is_collected() && Instant::now() < deadline {
                    thread::sleep(ENDING_CHECK);
                }
                group.send(OsSignal::KILL);
            });
        if killing.is_err() {
            self.send(OsSignal::KILL);
        }
    }
}

impl Child for ProcessGroup {
    fn signal(&self, signal: Signal) {
        self.send(os_signal_of(signal));
    }

    fn end(&self) {
        if self.is_collected() {
            return;
        }
        self.send(self.ending);
        self.kill_after_grace();
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
