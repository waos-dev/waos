//! A command's job: the job object every process the command starts is in, given to it as it is
//! created (`PROC_THREAD_ATTRIBUTE_JOB_LIST`), never assigned after, which would leave a moment
//! for a child to escape it. Ending the job ends them all, and WaOS letting go of the job, or
//! dying, ends them too (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).
//!
//! The invariant: a signal reaches the command only until its end has been collected; from then
//! on its job may hold processes it left behind, which only the job's close ends.

use std::io;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::os::windows::process::{CommandExt as _, ProcThreadAttributeList};
use std::process::Command as Process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{mem, ptr};

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use waos_capabilities::{Child, ExitStatus, Signal};
use windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, PROC_THREAD_ATTRIBUTE_JOB_LIST};

use super::pseudoconsole::ConsoleSignals;
use crate::pipes::StartedWithPipes;

/// The exit code a job's processes end with when the job is ended, as `TerminateProcess` ends one
/// that a signal killed.
const ENDED_BY_JOB: u32 = 1;

/// A job object of its own for one command.
pub(crate) struct Job {
    handle: OwnedHandle,
}

impl Job {
    /// A new job, which ends its processes when its last handle closes.
    pub(crate) fn new() -> io::Result<Job> {
        // SAFETY: no attributes and no name; the handle the call answers is this function's to own.
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a handle the call just opened, owned by nothing else.
        let job = Job {
            handle: unsafe { OwnedHandle::from_raw_handle(raw) },
        };
        // SAFETY: an all-zero limit information is a valid value, of which one flag is set.
        let mut limits = unsafe { mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the job's handle, and a structure of the class's type and size, which outlives
        // the call.
        let set = unsafe {
            SetInformationJobObject(
                job.handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                u32::try_from(mem::size_of_val(&limits)).expect("a structure's size"),
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// The job's handle, as a process's attribute list takes it.
    pub(crate) fn raw(&self) -> isize {
        self.handle.as_raw_handle() as isize
    }

    /// Ends every process in the job.
    pub(crate) fn end(&self) {
        // SAFETY: the job's handle. A job whose processes have all ended has nothing to end.
        unsafe { TerminateJobObject(self.handle.as_raw_handle(), ENDED_BY_JOB) };
    }
}

/// A started command as Windows knows it: its job, which ends it, its pseudoconsole where it runs
/// in one, which interrupts and hangs it up, and how it ended once that is collected.
#[derive(Clone)]
pub(crate) struct CommandJob {
    job: Arc<Job>,
    console: Option<Arc<ConsoleSignals>>,
    /// Whether the command's end has been collected.
    collected: Arc<AtomicBool>,
    exit: Shared<oneshot::Receiver<ExitStatus>>,
}

/// Where whoever collects a command's end says how it ended.
pub(crate) struct Collector {
    collected: Arc<AtomicBool>,
    ended: oneshot::Sender<ExitStatus>,
}

impl CommandJob {
    /// The command in `job`, run in the pseudoconsole `console` or with pipes, and what the one
    /// that collects its end tells it through.
    pub(crate) fn of(job: Job, console: Option<Arc<ConsoleSignals>>) -> (CommandJob, Collector) {
        let collected = Arc::new(AtomicBool::new(false));
        let (ended, exit) = oneshot::channel();
        let command = CommandJob {
            job: Arc::new(job),
            console,
            collected: Arc::clone(&collected),
            exit: exit.shared(),
        };
        (command, Collector { collected, ended })
    }
}

impl Child for CommandJob {
    fn signal(&self, signal: Signal) {
        if self.collected.load(Ordering::Acquire) {
            return;
        }
        match (signal, &self.console) {
            (Signal::Interrupt, Some(console)) => console.interrupt(),
            (Signal::Hangup, Some(console)) => console.hang_up(),
            _ => self.job.end(),
        }
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

/// Starts `process`, its three pipes piped, in a job of its own, given at its creation through
/// `std`'s attribute list, with a hidden console of its own: none of WaOS's, which may be the
/// terminal WaOS was run from. Its pipes are read and written on tokio's runtime, and its end waited
/// for on a thread, as `std` starts it, not tokio.
pub(crate) fn start_with_pipes(mut process: Process) -> io::Result<StartedWithPipes> {
    let job = Job::new()?;
    let jobs = [job.raw()];
    let attributes = ProcThreadAttributeList::build()
        .attribute(PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, &jobs)
        .finish()?;
    let mut child = process
        .creation_flags(CREATE_NO_WINDOW)
        .spawn_with_attributes(&attributes)?;
    let (input, output, errors) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let (group, collector) = CommandJob::of(job, None);
    Ok(StartedWithPipes {
        input: ChildStdin::from_std(input.expect("a piped input"))?,
        output: ChildStdout::from_std(output.expect("a piped output"))?,
        errors: ChildStderr::from_std(errors.expect("piped errors"))?,
        group,
        collector,
        end: end_of(child),
    })
}

/// How `child` ends, waited for on a thread of its own.
fn end_of(mut child: std::process::Child) -> BoxFuture<'static, ExitStatus> {
    let (ended, end) = oneshot::channel();
    let waiting = std::thread::Builder::new()
        .name("command exit".to_owned())
        .spawn(move || {
            let status = child.wait().ok().and_then(|status| status.code());
            let _ = ended
                .send(status.map_or(ExitStatus::Unknown, |code| exit_status_of_code(code as u32)));
        });
    match waiting {
        Ok(_) => end
            .map(|ended| ended.unwrap_or(ExitStatus::Unknown))
            .boxed(),
        Err(_) => async { ExitStatus::Unknown }.boxed(),
    }
}

/// How a process that exited with `code` ended: by the signal `INT` where Ctrl-C ended it, as a
/// POSIX shell on Windows reports `STATUS_CONTROL_C_EXIT`, and by its code otherwise.
pub(crate) fn exit_status_of_code(code: u32) -> ExitStatus {
    if code == STATUS_CONTROL_C_EXIT as u32 {
        ExitStatus::Signaled("INT".to_owned())
    } else {
        ExitStatus::Exited(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_end_by_ctrl_c_is_the_signal_int_and_any_other_its_code() {
        assert_eq!(
            exit_status_of_code(0xC000_013A),
            ExitStatus::Signaled("INT".to_owned())
        );
        assert_eq!(exit_status_of_code(3), ExitStatus::Exited(3));
        assert_eq!(exit_status_of_code(ENDED_BY_JOB), ExitStatus::Exited(1));
    }
}
