//! A run: the program built, installed from its source as its development install, launched and
//! attached to, in the WaOS on this machine (the design's decision 70), as `flutter run` installs,
//! launches and attaches. It runs with its development install's settings and folders, the
//! variables `--env` gave for this launch alone (decision 68) and the arguments after `--` as its
//! own. What it writes comes to this process's stdout and stderr as it writes it, each line after
//! the program's name; how it ended becomes the exit status; Ctrl-C, or the terminal closing,
//! stops it. `waos install` then `waos launch` is the same run, detached.
//!
//! The invariant: nothing here runs a program. It runs in the WaOS app, and this process holds
//! only the connections, so Ctrl-C stops it by asking WaOS to.

use std::process::ExitCode;

use anyhow::Result;
use waos_package::ProgramId;

use crate::attach::{self, OnInterrupt};
use crate::connecting::{WaOSStarter, connect_or_start};
use crate::install::{InstallBuild, install_on};
use crate::launch::launch_on;
use crate::{asking, stop};

/// Installs `build`, launches it with `variables` added to its environment and `args` as its
/// arguments, and attaches to it, each line of its output after `name`, until it ends, or until
/// Ctrl-C, which stops it.
pub async fn run(
    build: &InstallBuild,
    name: &str,
    variables: &[(String, String)],
    args: &[String],
    starter: &dyn WaOSStarter,
) -> Result<ExitCode> {
    let id = install_on(connect_or_start(starter).await?, build).await?;
    let launching = launch_on(asking::connect().await?, &id, name, args, variables);
    tokio::select! {
        launched = launching => launched?,
        () = attach::interrupted() => return stopped(&id, name).await,
    }
    attach::attach_on(asking::connect().await?, &id, name, OnInterrupt::Stop).await
}

/// Stops the program `id` a Ctrl-C ended the run of before it was attached to, and says so.
async fn stopped(id: &ProgramId, name: &str) -> Result<ExitCode> {
    stop::stop_quietly(id).await?;
    eprintln!("waos: {name} stopped");
    Ok(ExitCode::from(attach::INTERRUPTED))
}
