//! A run: the program sent to the WaOS on this machine over `waos-dev-protocol`'s connection, which is the
//! run (the design's decision 29), as the program its id names, which runs with that program's
//! settings and folders in WaOS. What the program writes comes to this process's stdout and
//! stderr as it writes it; what WaOS asks its user while the program waits for the answer is
//! said, with the row that answers it ahead; how it ended is said and becomes the exit status;
//! Ctrl-C closes the connection, which stops the program.
//!
//! The invariant: nothing here runs a program. It runs in the WaOS app, and this process holds
//! only the connection, so closing it is the one way to stop the run.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{FromWaOS, OutputStream, RunEnd, ToWaOS};
use waos_package::ProgramId;

use crate::asked;
use crate::build::BuiltProgram;
use crate::connecting::connect_or_start;

/// The exit status of a run Ctrl-C stopped: the shell's for a command the interrupt ended.
const INTERRUPTED: u8 = 130;

/// Runs `program` in WaOS, as the program `id` names, until it ends, or until Ctrl-C.
pub async fn run(id: ProgramId, program: &BuiltProgram) -> Result<ExitCode> {
    let component = std::fs::read(&program.component)
        .with_context(|| format!("reading {}", program.component.display()))?;
    let mut connection = connect_or_start().await?;
    connection
        .agree_on_protocol()
        .await
        .context("the WaOS that answered")?;
    let name = &program.name;
    connection
        .send(&ToWaOS::Run { id, component })
        .await
        .with_context(|| format!("sending {name} to WaOS"))?;
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    loop {
        tokio::select! {
            received = connection.receive::<FromWaOS>() => match received.context("reading from WaOS")? {
                Some(FromWaOS::Compiling) => eprintln!("cargo waos: WaOS is compiling {name}"),
                Some(FromWaOS::Started) => eprintln!("cargo waos: {name} is running in WaOS"),
                Some(FromWaOS::Output { stream, bytes }) => write_output(stream, &bytes)?,
                Some(FromWaOS::Ended(end)) => return Ok(ended(name, &end)),
                Some(FromWaOS::Refused(why)) => bail!("WaOS refused {name}: {why}"),
                Some(FromWaOS::Asking(asked)) => {
                    eprintln!("cargo waos: WaOS asks the user: {}", asked::said(&asked, name));
                }
                Some(other @ (FromWaOS::Opened
                | FromWaOS::Screenshots(_)
                | FromWaOS::InputDone { .. }
                | FromWaOS::Unreached(_)
                | FromWaOS::Settings(_)
                | FromWaOS::Resized { .. }
                | FromWaOS::Inspected(_))) => {
                    bail!("WaOS answered {other:?}, which a run never asks for")
                }
                None => bail!("WaOS closed the connection before {name} ended"),
            },
            _ = &mut interrupted => {
                drop(connection);
                eprintln!("cargo waos: {name} stopped");
                return Ok(ExitCode::from(INTERRUPTED));
            }
        }
    }
}

/// Writes `bytes` to this process's own stream of the same name, at once.
fn write_output(stream: OutputStream, bytes: &[u8]) -> Result<()> {
    let written = match stream {
        OutputStream::Stdout => io::stdout()
            .lock()
            .write_all(bytes)
            .and_then(|()| io::stdout().flush()),
        OutputStream::Stderr => io::stderr()
            .lock()
            .write_all(bytes)
            .and_then(|()| io::stderr().flush()),
    };
    written.context("writing the program's output")
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
    };
    eprintln!("cargo waos: {said}");
    status
}
