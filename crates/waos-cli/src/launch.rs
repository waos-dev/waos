//! `waos launch`'s side of a launch: an installed program started in the WaOS on this machine, in
//! place of its run if it runs, with the arguments after `--` and the variables `--env` gave for
//! this launch alone (the design's decisions 68 and 70); answered once it runs, after WaOS says it
//! is compiling where it compiles its code first. The program runs on in WaOS whatever this
//! process does next.
//!
//! The invariant: nothing here runs a program. It runs in the WaOS app; `waos attach` reads it
//! and `waos stop` ends it.

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{ClientStream, Connection, Launch, LaunchAnswer, Request};
use waos_package::ProgramId;

use crate::asking;

/// Launches the installed program `id`, `name` to its user, over `connection`, after saying its
/// hello, with `arguments` and `variables` for this launch alone; returns once it runs.
pub async fn launch_on(
    connection: Connection<ClientStream>,
    id: &ProgramId,
    name: &str,
    arguments: &[String],
    variables: &[(String, String)],
) -> Result<()> {
    let request = Request::Launch(Launch {
        id: id.clone(),
        arguments: arguments.to_vec(),
        variables: variables.to_vec(),
    });
    let mut connection = asking::greeted(connection, &request).await?;
    connection
        .send(&request)
        .await
        .with_context(|| format!("launching {name}"))?;
    loop {
        match connection
            .receive::<LaunchAnswer>()
            .await
            .context("reading from WaOS")?
        {
            Some(LaunchAnswer::Compiling) => eprintln!("waos: WaOS is compiling {name}"),
            Some(LaunchAnswer::Launched) => return Ok(()),
            Some(LaunchAnswer::Refused(refusal)) => {
                bail!("WaOS refused {name}: {}", refusal.message)
            }
            // An answer of a newer WaOS's this build does not know: skipped, as the protocol
            // promises.
            Some(LaunchAnswer::Unknown) => {}
            None => bail!("WaOS closed the connection before {name} was launched"),
        }
    }
}
