//! A command run on this computer in a terminal of its own, the system's (`system`): a
//! pseudo-terminal on Unix's systems, a pseudoconsole on Windows.
//!
//! A terminal's reads and writes block, and so does waiting for the command to end, so each has a
//! thread of its own, and the program hears from them through channels, never blocking its
//! thread.
//!
//! Output is read up to `READ_AHEAD` reads ahead of the program, as sshd reads a terminal ahead
//! into its channel's window. Past that the output thread waits, the terminal fills, and the
//! command waits on its next write, as one whose terminal nobody reads does. Without it a command
//! that printed a line and ended would wait on macOS until the line was read, since the last close
//! of a terminal there waits for its output to be read. Once the output is dropped, what the
//! command prints is thrown away, so it never waits on a reader that has gone.
//!
//! The invariant: dropping the terminal hangs the command up.

use std::io::{self, Read, Write};
use std::sync::mpsc as std_mpsc;
use std::thread;

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt, SinkExt, StreamExt};
use waos_capabilities::{Child, Command, ExecError, InTerminal, Terminal, TerminalSize};

use crate::login_environment::Variables;
use crate::starting::starting;
use crate::system::{self, TerminalSession};

/// The most one read of a terminal's output takes.
const CHUNK: usize = 16 * 1024;

/// How many reads of a terminal's output may wait for the program to take them.
const READ_AHEAD: usize = 64;

/// A command just started in a terminal: what the program signals and waits on, the terminal's
/// session, and its two ends' halves, what the command writes and where what is typed goes.
pub(crate) struct StartedTerminal {
    pub(crate) child: Box<dyn Child>,
    pub(crate) session: TerminalSession,
    pub(crate) reader: Box<dyn Read + Send>,
    pub(crate) writer: Box<dyn Write + Send>,
}

/// Runs `command` in a new terminal of `size`, in `login`, the login environment, with its
/// variables on top.
pub(crate) fn exec_in_terminal(
    command: &Command,
    size: TerminalSize,
    login: &Variables,
) -> Result<InTerminal, ExecError> {
    let started = system::start_in_terminal(starting(command, login), size)?;
    Ok(InTerminal {
        child: started.child,
        terminal: Box::new(LocalTerminal {
            input: write_on_a_thread(started.writer),
            session: started.session,
        }),
        output: read_on_a_thread(started.reader),
    })
}

/// A command's terminal on this computer.
struct LocalTerminal {
    /// Where typed input goes, for the input thread to write in the order it came.
    input: std_mpsc::Sender<TypedInput>,
    /// The terminal itself, which sets its size and hangs the command up.
    session: TerminalSession,
}

impl Terminal for LocalTerminal {
    fn write(&self, data: Vec<u8>) -> BoxFuture<'static, Result<(), ExecError>> {
        let (taken, answer) = oneshot::channel();
        let sent = self.input.send(TypedInput { bytes: data, taken });
        async move {
            sent.map_err(|_| input_thread_gone())?;
            answer.await.unwrap_or_else(|_| Err(input_thread_gone()))
        }
        .boxed()
    }

    fn resize(&self, size: TerminalSize) {
        self.session.resize(size);
    }
}

impl Drop for LocalTerminal {
    fn drop(&mut self) {
        self.session.hang_up();
    }
}

/// Bytes to write to the terminal, and where to say once it has taken them.
struct TypedInput {
    bytes: Vec<u8>,
    taken: oneshot::Sender<Result<(), ExecError>>,
}

/// Reads the terminal on a thread of its own, until it closes, into what this answers.
fn read_on_a_thread(mut reader: Box<dyn Read + Send>) -> BoxStream<'static, Vec<u8>> {
    let (chunks, output) = mpsc::channel(READ_AHEAD);
    spawn_thread("terminal output", move || {
        pass_on(&mut reader, chunks);
        throw_away(&mut reader);
    });
    output.boxed()
}

/// Passes what the terminal has to `chunks`, waiting while they are full, until the terminal
/// closes or nobody reads them.
fn pass_on(reader: &mut dyn Read, mut chunks: mpsc::Sender<Vec<u8>>) {
    while let Some(chunk) = next_chunk(reader) {
        if futures::executor::block_on(chunks.send(chunk)).is_err() {
            return;
        }
    }
}

/// Reads what the terminal has until it closes, and keeps none of it.
fn throw_away(reader: &mut dyn Read) {
    while next_chunk(reader).is_some() {}
}

/// What the terminal has next; none once it has closed.
fn next_chunk(reader: &mut dyn Read) -> Option<Vec<u8>> {
    let mut chunk = vec![0; CHUNK];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return None,
            Ok(read) => {
                chunk.truncate(read);
                return Some(chunk);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // Linux answers a terminal whose started end has closed with an error, not an end.
            Err(_) => return None,
        }
    }
}

/// Writes what is typed to the terminal on a thread of its own, in the order it came.
fn write_on_a_thread(mut writer: Box<dyn Write + Send>) -> std_mpsc::Sender<TypedInput> {
    let (input, typed) = std_mpsc::channel::<TypedInput>();
    spawn_thread("terminal input", move || {
        for TypedInput { bytes, taken } in typed {
            let written = writer.write_all(&bytes).and_then(|()| writer.flush());
            let _ = taken.send(written.map_err(|error| ExecError::Failed(error.to_string())));
        }
    });
    input
}

pub(crate) fn spawn_thread(name: &str, work: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(work)
        .expect("a thread for a terminal");
}

fn input_thread_gone() -> ExecError {
    ExecError::Failed("the terminal's input thread ended".to_owned())
}
