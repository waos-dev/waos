//! `waos attach`'s side of an attach: what a program running writes, however it was started, as
//! it writes it, to this process's stdout and stderr, each line after the program's name, so two programs'
//! output read apart, from what it wrote with nobody attached on; what WaOS asks its user while
//! the program waits for the answer, with the row that answers it ahead; and how it ended, which
//! becomes the exit status (the design's decision 70). Ctrl-C, or the terminal closing, detaches
//! and the program runs on, or, for `waos run`, stops it.
//!
//! The invariant: nothing here runs a program. It runs in the WaOS app, and this process holds
//! only the connection, which closing detaches from.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{ClientConnection, OutputStream, Request, RunEnd, RunEvent};
use waos_package::ProgramId;

use crate::destination::Destination;
use crate::{asked, asking, stop};

/// The exit status of an attach Ctrl-C ended: the shell's for a command the interrupt ended.
pub(crate) const INTERRUPTED: u8 = 130;

/// What Ctrl-C, or the terminal closing, does to the program attached to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnInterrupt {
    /// Detaches, as `waos attach`'s Ctrl-C does: the program runs on.
    Detach,
    /// Stops it, as `waos run`'s Ctrl-C does.
    Stop,
}

/// Attaches over `connection` to `destination` to the program `id`, `name` to its user, after
/// saying its hello, and writes what it writes until it ends, or until Ctrl-C, which does what
/// `on_interrupt` says.
pub async fn attach_on(
    connection: ClientConnection,
    destination: &Destination,
    id: &ProgramId,
    name: &str,
    on_interrupt: OnInterrupt,
) -> Result<ExitCode> {
    let request = Request::Attach { id: id.clone() };
    let mut connection = asking::greeted(connection, destination, &request).await?;
    connection
        .send(&request)
        .await
        .with_context(|| format!("attaching to {name}"))?;
    let interrupted = interrupted();
    tokio::pin!(interrupted);
    let mut output = Output {
        name,
        stdout_at_line_start: true,
        stderr_at_line_start: true,
    };
    loop {
        tokio::select! {
            received = connection.receive::<RunEvent>() => match received.context("reading from WaOS")? {
                Some(RunEvent::Started) => eprintln!("waos: {name} is running in WaOS"),
                Some(RunEvent::Output { stream, bytes }) => output.written(stream, &bytes)?,
                Some(RunEvent::Ended(end)) => return Ok(ended(name, &end)),
                Some(RunEvent::Refused(refusal)) => {
                    bail!("WaOS refused {name}: {}", refusal.message)
                }
                Some(RunEvent::Asking(asked)) => {
                    eprintln!("waos: WaOS asks the user: {}", asked::said(&asked, name));
                }
                // An event of a newer WaOS's this build does not know: skipped, as the protocol
                // promises, never breaking the attach over it.
                Some(RunEvent::Unknown) => {}
                None => bail!("WaOS closed the connection before {name} ended"),
            },
            () = &mut interrupted => {
                drop(connection);
                return interrupt(destination, id, name, on_interrupt).await;
            }
        }
    }
}

/// What Ctrl-C comes to: the program at `destination` stopped or left running, as `on_interrupt`
/// says, and said.
async fn interrupt(
    destination: &Destination,
    id: &ProgramId,
    name: &str,
    on_interrupt: OnInterrupt,
) -> Result<ExitCode> {
    match on_interrupt {
        OnInterrupt::Stop => {
            stop::stop_quietly(destination, id).await?;
            eprintln!("waos: {name} stopped");
        }
        OnInterrupt::Detach => {
            eprintln!("waos: detached from {name}, which runs on; `waos stop` stops it");
        }
    }
    Ok(ExitCode::from(INTERRUPTED))
}

/// Resolves at Ctrl-C, or, on Unix, when the terminal closes (`SIGHUP`) or the process is asked
/// to end (`SIGTERM`), and on Windows when the console closes.
pub(crate) async fn interrupted() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut hangup), Ok(mut terminate)) = (
            signal(SignalKind::hangup()),
            signal(SignalKind::terminate()),
        ) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = hangup.recv() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(windows)]
    {
        let Ok(mut closing) = tokio::signal::windows::ctrl_close() else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = closing.recv() => {}
        }
    }
}

/// How a program's output reaches this terminal: each line of the program's, after the program's
/// name, so two programs' output read apart.
struct Output<'a> {
    name: &'a str,
    /// Whether the next byte of each stream starts a line.
    stdout_at_line_start: bool,
    stderr_at_line_start: bool,
}

impl Output<'_> {
    /// Writes `bytes` to this process's own `stream` at once, each line after the program's name.
    fn written(&mut self, stream: OutputStream, bytes: &[u8]) -> Result<()> {
        let (prefixed, at_line_start) = match stream {
            OutputStream::Stdout => prefixed(self.name, self.stdout_at_line_start, bytes),
            // Unknown streams fall back to stderr so bytes from a newer WaOS are still shown.
            OutputStream::Stderr | OutputStream::Unknown => {
                prefixed(self.name, self.stderr_at_line_start, bytes)
            }
        };
        match stream {
            OutputStream::Stdout => self.stdout_at_line_start = at_line_start,
            OutputStream::Stderr | OutputStream::Unknown => {
                self.stderr_at_line_start = at_line_start
            }
        }
        let written = match stream {
            OutputStream::Stdout => io::stdout()
                .lock()
                .write_all(&prefixed)
                .and_then(|()| io::stdout().flush()),
            OutputStream::Stderr | OutputStream::Unknown => io::stderr()
                .lock()
                .write_all(&prefixed)
                .and_then(|()| io::stderr().flush()),
        };
        written.context("writing the program's output")
    }
}

/// `bytes` with `name` and `: ` before each line, and whether its last byte ended one; a piece of
/// a line, as a program writes one, starts at `at_line_start` as the piece before it left it.
fn prefixed(name: &str, at_line_start: bool, bytes: &[u8]) -> (Vec<u8>, bool) {
    let mut prefixed = Vec::with_capacity(bytes.len());
    let mut at_line_start = at_line_start;
    for &byte in bytes {
        if at_line_start {
            prefixed.extend_from_slice(name.as_bytes());
            prefixed.extend_from_slice(b": ");
            at_line_start = false;
        }
        prefixed.push(byte);
        if byte == b'\n' {
            at_line_start = true;
        }
    }
    (prefixed, at_line_start)
}

/// Says how `name` ended, and answers the exit status that says the same.
fn ended(name: &str, end: &RunEnd) -> ExitCode {
    let (said, status) = match end {
        RunEnd::Returned { success: true } => (format!("{name} returned"), ExitCode::SUCCESS),
        RunEnd::Returned { success: false } => {
            (format!("{name} returned failure"), ExitCode::FAILURE)
        }
        RunEnd::Failed(error) => (format!("{name} failed: {error}"), ExitCode::FAILURE),
        RunEnd::Stopped => (format!("{name} was stopped by WaOS"), ExitCode::FAILURE),
        RunEnd::Replaced => (
            format!("{name} was replaced by a later launch or install"),
            ExitCode::FAILURE,
        ),
        RunEnd::Unknown => (
            format!("{name} ended in a way this waos does not know; update waos"),
            ExitCode::FAILURE,
        ),
    };
    eprintln!("waos: {said}");
    status
}

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{Hello, Listener, TestPlace};

    use super::*;

    #[test]
    fn each_line_comes_after_the_programs_name_as_the_pieces_of_it_arrive() {
        assert_eq!(
            prefixed("notes", true, b"hello\nworld"),
            (b"notes: hello\nnotes: world".to_vec(), false)
        );
        assert_eq!(prefixed("notes", false, b"!\n"), (b"!\n".to_vec(), true));
        assert_eq!(prefixed("notes", false, b"\n"), (b"\n".to_vec(), true));
        assert_eq!(prefixed("notes", true, b""), (Vec::new(), true));
    }

    #[tokio::test]
    async fn unknown_events_are_skipped_and_an_end_this_build_does_not_know_fails() {
        struct CborBytes(&'static [u8]);
        impl serde::Serialize for CborBytes {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(self.0)
            }
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum NewerRunEvent {
            Progress {
                percent: u8,
            },
            Output {
                stream: NewerStream,
                bytes: CborBytes,
            },
            Ended(NewerEnd),
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum NewerStream {
            Trace,
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum NewerEnd {
            Cancelled,
        }

        let place = TestPlace::new("attach-1");
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            server
                .accept_hello(&Hello::waos("WaOS 0.3.0"))
                .await
                .unwrap();
            let Some(Request::Attach { id }) = server.receive::<Request>().await.unwrap() else {
                panic!("an attach");
            };
            assert_eq!(id.as_str(), "dev.example.notes.development");
            server.send(&RunEvent::Started).await.unwrap();
            server
                .send(&NewerRunEvent::Progress { percent: 50 })
                .await
                .unwrap();
            server
                .send(&NewerRunEvent::Output {
                    stream: NewerStream::Trace,
                    bytes: CborBytes(b"from future stream\n"),
                })
                .await
                .unwrap();
            server
                .send(&NewerRunEvent::Ended(NewerEnd::Cancelled))
                .await
                .unwrap();
        });
        let client = ClientConnection::connect(&socket).await.unwrap();
        let id: ProgramId = "dev.example.notes.development".parse().unwrap();
        let destination = Destination::ThisMachine { socket };
        let status = attach_on(client, &destination, &id, "notes", OnInterrupt::Detach)
            .await
            .unwrap();
        assert_eq!(
            status,
            ExitCode::FAILURE,
            "unknown RunEnd fails rather than silently succeeding"
        );
        served.await.unwrap();
    }

    #[tokio::test]
    async fn an_end_in_failure_sets_the_exit_status() {
        let place = TestPlace::new("attach-2");
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            server
                .accept_hello(&Hello::waos("WaOS 0.3.0"))
                .await
                .unwrap();
            server.receive::<Request>().await.unwrap();
            server.send(&RunEvent::Started).await.unwrap();
            server
                .send(&RunEvent::Ended(RunEnd::Returned { success: false }))
                .await
                .unwrap();
        });
        let client = ClientConnection::connect(&socket).await.unwrap();
        let id: ProgramId = "dev.example.notes.development".parse().unwrap();
        let destination = Destination::ThisMachine { socket };
        let status = attach_on(client, &destination, &id, "notes", OnInterrupt::Stop)
            .await
            .unwrap();
        assert_eq!(status, ExitCode::FAILURE);
        served.await.unwrap();
    }
}
