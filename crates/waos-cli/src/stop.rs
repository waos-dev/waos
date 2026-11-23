//! A stop: the program running in the WaOS on this machine ended, however it was started, as
//! Ctrl-C in `waos run` ends it.
//!
//! The invariant: a stop never starts WaOS. With none answering, nothing runs to stop.

use anyhow::Result;
use waos_dev_protocol::{Request, StopAnswer};
use waos_package::ProgramId;

use crate::asking::ask;

/// Stops the program `id` if it runs, saying nothing: what Ctrl-C does to `waos run`'s program.
pub async fn stop_quietly(id: &ProgramId) -> Result<()> {
    ask::<StopAnswer>(&Request::Stop { id: id.clone() }).await?;
    Ok(())
}
