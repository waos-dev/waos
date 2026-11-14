//! A command's terminal on Windows: a pseudoconsole over two pipes, the command started on it by
//! WaOS itself (`CreateProcessW` with the pseudoconsole and the command's job among its
//! attributes), as Windows Terminal starts its shells, the job given at creation. Written after
//! Microsoft's "Creating a Pseudoconsole session" and portable-pty's `win/psuedocon.rs`, whose
//! standard handles it sets as invalid, so a command never writes to WaOS's own redirected ones.
//!
//! The command's console stays open after it ends until the pseudoconsole closes, and what it
//! printed reaches the output pipe only then: the pseudoconsole is closed as the command's end is
//! collected, so its output ends as a terminal's does when its command ends.
//!
//! The invariants a later editor will break:
//! - The pseudoconsole is closed once, whoever closes it first: the command's end, a hang-up or
//!   the terminal dropped.
//! - The pipes' ends the console holds are closed here as soon as the console has them, or the
//!   output would never end.

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Write as _};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::sync::{Arc, Mutex, PoisonError};
use std::{mem, ptr};

use waos_capabilities::{ExecError, ExitStatus, TerminalSize};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, S_OK, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use super::command_line::{command_line, environment_block, resolved_program};
use super::job::{Collector, CommandJob, Job, exit_status_of_code};
use crate::starting::Starting;
use crate::terminal::{StartedTerminal, spawn_thread};

/// What a terminal's ETX, Ctrl-C, is as typed.
const CTRL_C: u8 = 3;

/// A pseudoconsole, closed once.
struct Pseudoconsole {
    handle: Mutex<Option<HPCON>>,
}

impl Pseudoconsole {
    /// A pseudoconsole of `size` reading what is typed from `typed` and writing what it shows to
    /// `shown`, the pipes' console ends, which it keeps copies of.
    fn open(
        size: TerminalSize,
        typed: &OwnedHandle,
        shown: &OwnedHandle,
    ) -> io::Result<Pseudoconsole> {
        let mut handle: HPCON = 0;
        // SAFETY: two open pipe handles, which the call duplicates; the pseudoconsole it answers
        // is closed once, by `close`.
        let made = unsafe {
            CreatePseudoConsole(
                coordinates_of(size),
                typed.as_raw_handle(),
                shown.as_raw_handle(),
                0,
                &mut handle,
            )
        };
        if made != S_OK {
            return Err(io::Error::from_raw_os_error(made));
        }
        Ok(Pseudoconsole {
            handle: Mutex::new(Some(handle)),
        })
    }

    fn resize(&self, size: TerminalSize) {
        if let Some(handle) = *self.handle.lock().unwrap_or_else(PoisonError::into_inner) {
            // SAFETY: an open pseudoconsole. One that refuses the size keeps its own.
            unsafe { ResizePseudoConsole(handle, coordinates_of(size)) };
        }
    }

    /// Closes the pseudoconsole, which closes the console of every process attached to it, and
    /// ends its output once what it showed is read; nothing where it is closed already.
    fn close(&self) {
        let closed = self
            .handle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = closed {
            // SAFETY: an open pseudoconsole, closed once: its handle was just taken.
            unsafe { ClosePseudoConsole(handle) };
        }
    }

    fn raw(&self) -> Option<HPCON> {
        *self.handle.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Pseudoconsole {
    fn drop(&mut self) {
        self.close();
    }
}

/// What a command's pseudoconsole does in place of signals: its Ctrl-C typed, and its close.
pub(crate) struct ConsoleSignals {
    pseudoconsole: Arc<Pseudoconsole>,
    /// The typed end of the input pipe, beside the one the input thread writes through.
    typed: Mutex<File>,
}

impl ConsoleSignals {
    /// Types Ctrl-C, which the console turns into a Ctrl-C for the processes attached to it.
    pub(crate) fn interrupt(&self) {
        let mut typed = self.typed.lock().unwrap_or_else(PoisonError::into_inner);
        // A console that has closed takes nothing more.
        let _ = typed.write_all(&[CTRL_C]);
    }

    /// Closes the command's console, as a terminal window closing does.
    pub(crate) fn hang_up(&self) {
        self.pseudoconsole.close();
    }
}

/// A command's pseudoconsole, which sets its size and closes it.
pub(crate) struct TerminalSession {
    pseudoconsole: Arc<Pseudoconsole>,
}

impl TerminalSession {
    /// Tells the command the terminal's new size.
    pub(crate) fn resize(&self, size: TerminalSize) {
        self.pseudoconsole.resize(size);
    }

    /// Closes the command's console, as the terminal closes.
    pub(crate) fn hang_up(&self) {
        self.pseudoconsole.close();
    }
}

/// Starts `starting` in a new pseudoconsole of `size`, in a job of its own.
pub(crate) fn start_in_terminal(
    starting: Starting,
    size: TerminalSize,
) -> Result<StartedTerminal, ExecError> {
    let (typed_by_console, typed) = pipe().map_err(failed)?;
    let (shown, shown_by_console) = pipe().map_err(failed)?;
    let pseudoconsole =
        Arc::new(Pseudoconsole::open(size, &typed_by_console, &shown_by_console).map_err(failed)?);
    drop((typed_by_console, shown_by_console));
    let job = Job::new().map_err(failed)?;
    let process = start_attached(&starting, &pseudoconsole, &job)
        .map_err(|error| ExecError::Refused(format!("{}: {error}", starting.program)))?;
    let typed = File::from(typed);
    let signals = ConsoleSignals {
        pseudoconsole: Arc::clone(&pseudoconsole),
        typed: Mutex::new(typed.try_clone().map_err(failed)?),
    };
    let (command, collector) = CommandJob::of(job, Some(Arc::new(signals)));
    wait_on_a_thread(process, Arc::clone(&pseudoconsole), collector);
    Ok(StartedTerminal {
        child: Box::new(command),
        session: TerminalSession { pseudoconsole },
        reader: Box::new(File::from(shown)),
        writer: Box::new(typed),
    })
}

/// A pipe: its reading end, then its writing end.
fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let (mut reading, mut writing): (HANDLE, HANDLE) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: two out-pointers of the call's own; the handles it answers are owned below.
    if unsafe { CreatePipe(&mut reading, &mut writing, ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: handles the call just opened, owned by nothing else.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(reading),
            OwnedHandle::from_raw_handle(writing),
        )
    })
}

/// Starts `starting` attached to `pseudoconsole` and in `job`, both given to it as it is created;
/// answers the process's handle.
fn start_attached(
    starting: &Starting,
    pseudoconsole: &Pseudoconsole,
    job: &Job,
) -> io::Result<OwnedHandle> {
    let console = pseudoconsole
        .raw()
        .ok_or_else(|| io::Error::other("the pseudoconsole closed before the command started"))?;
    let jobs = [job.raw()];
    let mut attributes = AttributeList::new(&[
        (
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            // The pseudoconsole's value is the handle itself, not a pointer to it.
            console as *const c_void,
            mem::size_of::<HPCON>(),
        ),
        (
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            jobs.as_ptr().cast(),
            mem::size_of_val(&jobs),
        ),
    ])?;
    let program = resolved_program(&starting.program, &starting.variables);
    let mut line = command_line(&program, &starting.arguments);
    let mut environment = environment_block(&starting.variables);
    let folder: Vec<u16> = starting
        .folder
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: an all-zero `STARTUPINFOEXW` is a valid value, filled in below.
    let mut startup = unsafe { mem::zeroed::<STARTUPINFOEXW>() };
    startup.StartupInfo.cb = u32::try_from(mem::size_of::<STARTUPINFOEXW>()).expect("a size");
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.raw();
    // SAFETY: an all-zero `PROCESS_INFORMATION` is a valid value, which the call fills in.
    let mut started = unsafe { mem::zeroed::<PROCESS_INFORMATION>() };
    // SAFETY: a NUL-terminated command line the call may change, an environment block and a folder
    // that outlive the call, and the attribute list, whose values live in this function's frame.
    let created = unsafe {
        CreateProcessW(
            ptr::null(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            environment.as_mut_ptr().cast(),
            folder.as_ptr(),
            &startup.StartupInfo,
            &mut started,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the handles the call opened, owned by nothing else; the thread's closes at once.
    let (process, _thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(started.hProcess),
            OwnedHandle::from_raw_handle(started.hThread),
        )
    };
    Ok(process)
}

/// A process's attribute list, its values borrowed from the caller's frame.
struct AttributeList {
    memory: Vec<u64>,
}

impl AttributeList {
    /// A list of `attributes`, each its number, a pointer to its value and the value's size.
    fn new(attributes: &[(u32, *const c_void, usize)]) -> io::Result<AttributeList> {
        let count = u32::try_from(attributes.len()).expect("a few attributes");
        let mut bytes = 0;
        // SAFETY: asked with no list, the call only says how many bytes one needs, failing as
        // the buffer is too small, which is expected.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &mut bytes) };
        let mut list = AttributeList {
            memory: vec![0; bytes.div_ceil(8)],
        };
        // SAFETY: memory of `bytes` bytes at least, aligned for the list's pointers.
        if unsafe { InitializeProcThreadAttributeList(list.raw(), count, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        for &(attribute, value, size) in attributes {
            // SAFETY: an initialized list, and a value of `size` bytes, which the caller keeps
            // alive while the list is used.
            let updated = unsafe {
                UpdateProcThreadAttribute(
                    list.raw(),
                    0,
                    attribute as usize,
                    value,
                    size,
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            if updated == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(list)
    }

    fn raw(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.memory.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: a list initialized by `new`, deleted once.
        unsafe { DeleteProcThreadAttributeList(self.raw()) };
    }
}

/// Waits for `process` to end on a thread of its own, closes its pseudoconsole, so that what it
/// printed reaches the output and the output ends, and tells `collector` how it ended.
fn wait_on_a_thread(process: OwnedHandle, pseudoconsole: Arc<Pseudoconsole>, collector: Collector) {
    spawn_thread("terminal exit", move || {
        let status = wait_for_end_of(&process);
        pseudoconsole.close();
        collector.collected(status);
    });
}

/// Waits for `process` to end, and reads how it did.
fn wait_for_end_of(process: &OwnedHandle) -> ExitStatus {
    // SAFETY: an open process handle.
    if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
        return ExitStatus::Unknown;
    }
    let mut code = 0;
    // SAFETY: an open process handle, and an out-pointer of the call's own.
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return ExitStatus::Unknown;
    }
    exit_status_of_code(code)
}

fn coordinates_of(size: TerminalSize) -> COORD {
    let cells = |count: u16| i16::try_from(count).unwrap_or(i16::MAX).max(1);
    COORD {
        X: cells(size.columns),
        Y: cells(size.rows),
    }
}

fn failed(error: io::Error) -> ExecError {
    ExecError::Failed(error.to_string())
}
