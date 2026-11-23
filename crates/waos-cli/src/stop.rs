//! `waos stop`: the program running in the WaOS on this machine ended, however it was started,
//! as Ctrl-C in `waos run` ends it.
//!
//! The invariant: a stop never starts WaOS. With none answering, nothing runs to stop.

use std::process::ExitCode;

use anyhow::{Result, bail};
use waos_dev_protocol::{Request, StopAnswer};
use waos_package::ProgramId;

use crate::asking::ask;

/// Stops the program `id`, and says so; one that does not run is said, with the exit status 1.
pub async fn stop(id: ProgramId) -> Result<ExitCode> {
    match ask(&Request::Stop { id: id.clone() }).await? {
        StopAnswer::Stopped => {
            eprintln!("waos: {id} stopped");
            Ok(ExitCode::SUCCESS)
        }
        StopAnswer::Refused(refusal) => {
            eprintln!("waos: {}", refusal.message);
            Ok(ExitCode::FAILURE)
        }
        StopAnswer::Unknown => bail!(
            "WaOS answered the stop in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
    }
}

/// Stops the program `id` if it runs, saying nothing: what Ctrl-C does to `waos run`'s program.
pub async fn stop_quietly(id: &ProgramId) -> Result<()> {
    ask::<StopAnswer>(&Request::Stop { id: id.clone() }).await?;
    Ok(())
}
