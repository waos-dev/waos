//! `waos uninstall`: an installed program removed from the WaOS on this machine, with its settings
//! and its own folders, its run stopped first, as the library removes one; a development install
//! by default, as `waos install` made it.
//!
//! The invariant: what is removed is the one program the id names; the program published under
//! the plain id stays when its development install goes.

use std::process::ExitCode;

use anyhow::{Result, bail};
use waos_dev_protocol::{Request, UninstallAnswer};
use waos_package::ProgramId;

use crate::asking::ask_starting;
use crate::connecting::WaOSStarter;
use crate::destination::Destination;

/// Uninstalls the program `id`, in the WaOS running or the one started when none answers, and says
/// so; one not installed is said, with the exit status 1.
pub async fn uninstall(
    destination: &Destination,
    id: ProgramId,
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let request = Request::Uninstall { id: id.clone() };
    match ask_starting(destination, &request, starter).await? {
        UninstallAnswer::Uninstalled => {
            eprintln!("waos: {id} uninstalled, with its settings and its data");
            Ok(ExitCode::SUCCESS)
        }
        UninstallAnswer::Refused(refusal) => {
            eprintln!("waos: {}", refusal.message);
            Ok(ExitCode::FAILURE)
        }
        UninstallAnswer::Unknown => bail!(
            "WaOS answered the uninstall in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
    }
}
