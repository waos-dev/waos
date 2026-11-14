//! A command run on this computer with pipes: its input, output and errors each a pipe of its own,
//! on the runtime of the program that ran it. It starts in a group of its own, the system's
//! (`system`), which takes the signals sent to it.
//!
//! The invariant: the input is written in the order given, and closed once closed or dropped,
//! after what was given before; the command's end is collected by one task, which every caller
//! of its exit status hears.

use std::process::{Command as Process, Stdio};

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use waos_capabilities::{ChildInput, Command, ExecError, ExitStatus, WithPipes};

use crate::login_environment::Variables;
use crate::starting::{Starting, starting};
use crate::system::{self, Collector, CommandGroup};

/// The most one read of a pipe takes.
const CHUNK: usize = 16 * 1024;

/// A command just started with pipes: its three pipes, its group, what collects its end, and its
/// end, once it comes.
pub(crate) struct StartedWithPipes {
    pub(crate) input: ChildStdin,
    pub(crate) output: ChildStdout,
    pub(crate) errors: ChildStderr,
    pub(crate) group: CommandGroup,
    pub(crate) collector: Collector,
    pub(crate) end: BoxFuture<'static, ExitStatus>,
}

/// Runs `command` with pipes, in `login`, the login environment, with its variables on top.
pub(crate) fn exec(command: &Command, login: &Variables) -> Result<WithPipes, ExecError> {
    let Starting {
        program,
        arguments,
        variables,
        folder,
    } = starting(command, login);
    let mut process = Process::new(program);
    process
        .args(arguments)
        .env_clear()
        .envs(variables)
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let StartedWithPipes {
        input,
        output,
        errors,
        group,
        collector,
        end,
    } = system::start_with_pipes(process)
        .map_err(|error| ExecError::Refused(format!("{}: {error}", command.program)))?;
    tokio::spawn(async move { collector.collected(end.await) });
    Ok(WithPipes {
        child: Box::new(group),
        input: Box::new(PipeInput::writing(input)),
        output: chunks_of(output),
        errors: chunks_of(errors),
    })
}

/// What `pipe` gives, in chunks, until it closes or fails.
fn chunks_of(pipe: impl AsyncRead + Send + Unpin + 'static) -> BoxStream<'static, Vec<u8>> {
    futures::stream::unfold(pipe, |mut pipe| async move {
        let mut chunk = vec![0; CHUNK];
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => None,
            Ok(read) => {
                chunk.truncate(read);
                Some((chunk, pipe))
            }
        }
    })
    .boxed()
}

/// A command's input, written by a task of its own in the order given.
struct PipeInput {
    writes: mpsc::UnboundedSender<Write>,
}

/// Bytes to write, and where to say once the pipe has taken them.
struct Write {
    bytes: Vec<u8>,
    taken: oneshot::Sender<Result<(), ExecError>>,
}

impl PipeInput {
    /// Writes what is given to `pipe`, until closed.
    fn writing(mut pipe: ChildStdin) -> PipeInput {
        let (writes, mut given) = mpsc::unbounded::<Write>();
        tokio::spawn(async move {
            while let Some(Write { bytes, taken }) = given.next().await {
                let written = match pipe.write_all(&bytes).await {
                    Ok(()) => pipe.flush().await,
                    Err(error) => Err(error),
                };
                let _ = taken.send(written.map_err(|error| ExecError::Failed(error.to_string())));
            }
            // The pipe closes as it goes, after everything given before the close.
        });
        PipeInput { writes }
    }
}

impl ChildInput for PipeInput {
    fn write(&self, data: Vec<u8>) -> BoxFuture<'static, Result<(), ExecError>> {
        let (taken, answer) = oneshot::channel();
        let given = self.writes.unbounded_send(Write { bytes: data, taken });
        async move {
            given.map_err(|_| closed())?;
            answer.await.unwrap_or_else(|_| Err(closed()))
        }
        .boxed()
    }

    fn close(&self) {
        self.writes.close_channel();
    }
}

fn closed() -> ExecError {
    ExecError::Failed("the command's input is closed".to_owned())
}
