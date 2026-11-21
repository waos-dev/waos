//! A run: the program sent to the WaOS on this machine over `waos-dev-protocol`'s connection, which is the
//! run (the design's decision 29), as the program its id names, which runs with that program's
//! settings and folders in WaOS, the variables `--env` gave for it alone (decision 68) and the
//! arguments after `--` as its own. The component goes over in parts when WaOS does not hold it
//! already, so a component of any size runs; the run says how much it is sending when that takes
//! noticeable time. What
//! the program writes comes to this process's stdout and stderr as it writes it, each line after
//! the program's name, so two runs' output read apart; what WaOS asks its user while the program
//! waits for the answer is said, with the row that answers it ahead; how it ended is said and
//! becomes the exit status; Ctrl-C closes the connection, which stops the program.
//!
//! The invariant: nothing here runs a program. It runs in the WaOS app, and this process holds
//! only the connection, so closing it is the one way to stop the run.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{
    ClientStream, Connection, OutputStream, Request, Run, RunEnd, RunEvent, Upload,
    component_digest,
};
use waos_package::ProgramId;

use crate::asked;
use crate::asking;
use crate::build::BuiltProgram;
use crate::connecting::{WaOSStarter, connect_or_start};

/// The exit status of a run Ctrl-C stopped: the shell's for a command the interrupt ended.
const INTERRUPTED: u8 = 130;

/// Over this many bytes, sending a component takes noticeable time, so the run says how much it
/// is sending. A debug build is hundreds of megabytes; a release one is under this.
const SAID_OVER_BYTES: usize = 64 << 20;

/// Runs `program` in WaOS, as the program `id` names, with `variables` added to its environment
/// and `args` as its arguments, until it ends, or until Ctrl-C.
pub async fn run(
    id: ProgramId,
    program: &BuiltProgram,
    variables: &[(String, String)],
    args: &[String],
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let component = std::fs::read(&program.component)
        .with_context(|| format!("reading {}", program.component.display()))?;
    run_on(
        connect_or_start(starter).await?,
        id,
        &program.name,
        &component,
        variables,
        args,
    )
    .await
}

/// Runs `component` over `connection` as the program `id` (`name`), after saying its hello.
async fn run_on(
    connection: Connection<ClientStream>,
    id: ProgramId,
    name: &str,
    component: &[u8],
    variables: &[(String, String)],
    args: &[String],
) -> Result<ExitCode> {
    let request = Request::Run(Run {
        id,
        args: args.to_vec(),
        variables: variables.to_vec(),
        length: component.len() as u64,
        digest: component_digest(component),
    });
    let mut connection = asking::greeted(connection, &request).await?;
    send_run(&mut connection, name, &request, component).await?;
    stream_events(connection, name).await
}

/// Announces `request` to WaOS and sends `component` unless WaOS answers `Upload::Have`.
async fn send_run(
    connection: &mut Connection<ClientStream>,
    name: &str,
    request: &Request,
    component: &[u8],
) -> Result<()> {
    connection
        .send(request)
        .await
        .with_context(|| format!("sending {name} to WaOS"))?;
    match connection
        .receive::<Upload>()
        .await
        .context("reading from WaOS")?
    {
        Some(Upload::Send) => {
            if component.len() > SAID_OVER_BYTES {
                eprintln!(
                    "waos: sending {name} to WaOS, {} MiB",
                    component.len() >> 20
                );
            }
            connection
                .send_component(component)
                .await
                .with_context(|| format!("sending {name} to WaOS"))
        }
        // WaOS holds this component already: nothing is sent, and the run starts from it.
        Some(Upload::Have) => Ok(()),
        Some(Upload::Refused(refusal)) => bail!("WaOS refused {name}: {}", refusal.message),
        Some(Upload::Unknown) => bail!(
            "WaOS answered the upload in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
        None => bail!("WaOS closed the connection before {name} was sent"),
    }
}

/// Reads the running program's events until it ends or the user stops it with Ctrl-C.
async fn stream_events(mut connection: Connection<ClientStream>, name: &str) -> Result<ExitCode> {
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    let mut output = Output {
        name,
        stdout_at_line_start: true,
        stderr_at_line_start: true,
    };
    loop {
        tokio::select! {
            received = connection.receive::<RunEvent>() => match received.context("reading from WaOS")? {
                Some(RunEvent::Compiling) => eprintln!("waos: WaOS is compiling {name}"),
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
                // promises, never breaking the run over it.
                Some(RunEvent::Unknown) => {}
                None => bail!("WaOS closed the connection before {name} ended"),
            },
            _ = &mut interrupted => {
                drop(connection);
                eprintln!("waos: {name} stopped");
                return Ok(ExitCode::from(INTERRUPTED));
            }
        }
    }
}

/// How a run's output reaches this terminal: each line of the program's, after the program's
/// name, so two runs' output read apart.
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
            format!("{name} was replaced by a later run"),
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
    use waos_dev_protocol::{Component, Hello, Listener, TestPlace};

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
    async fn have_skips_the_upload_and_unknown_events_are_skipped() {
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

        let place = TestPlace::new("run-1");
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let component = b"component bytes".to_vec();
        let expected_digest = component_digest(&component);
        let expected_length = component.len() as u64;
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            server
                .accept_hello(&Hello::waos("WaOS 0.3.0"))
                .await
                .unwrap();
            let Some(Request::Run(run)) = server.receive::<Request>().await.unwrap() else {
                panic!("a run");
            };
            assert_eq!(run.args, ["--port", "8080"]);
            assert_eq!((run.length, run.digest), (expected_length, expected_digest));
            server.send(&Upload::Have).await.unwrap();
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
            // Nothing was uploaded after `Upload::Have`.
            server.receive::<Component>().await.unwrap()
        });
        let client = Connection::connect(&socket).await.unwrap();
        let status = run_on(
            client,
            "dev.example.notes".parse().unwrap(),
            "notes",
            &component,
            &[],
            &["--port".to_owned(), "8080".to_owned()],
        )
        .await
        .unwrap();
        assert_eq!(
            status,
            ExitCode::FAILURE,
            "unknown RunEnd fails rather than silently succeeding"
        );
        assert_eq!(served.await.unwrap(), None, "no component frame sent");
    }

    #[tokio::test]
    async fn send_uploads_the_component_and_ended_failure_sets_exit_status() {
        let place = TestPlace::new("run-2");
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let component = b"component bytes".to_vec();
        let expected = component.clone();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            server
                .accept_hello(&Hello::waos("WaOS 0.3.0"))
                .await
                .unwrap();
            let Some(Request::Run(run)) = server.receive::<Request>().await.unwrap() else {
                panic!("a run");
            };
            server.send(&Upload::Send).await.unwrap();
            let received = server
                .receive_component(run.length, &run.digest)
                .await
                .unwrap();
            assert_eq!(received, expected);
            server.send(&RunEvent::Started).await.unwrap();
            server
                .send(&RunEvent::Ended(RunEnd::Returned { success: false }))
                .await
                .unwrap();
        });
        let client = Connection::connect(&socket).await.unwrap();
        let status = run_on(
            client,
            "dev.example.notes".parse().unwrap(),
            "notes",
            &component,
            &[],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(status, ExitCode::FAILURE);
        served.await.unwrap();
    }
}
