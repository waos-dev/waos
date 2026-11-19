//! `waos open <file>... --program <id>` as a client of the WaOS on this machine: the files sent
//! over `waos-dev-protocol`'s socket, as `cargo waos run` reaches it, to be opened with the program the id
//! names (`opening` answers there), and its answer waited for.
//!
//! The invariant: the command ends only once a WaOS has answered, or once it knows none listens,
//! so whoever ran it hears whether the files reached the program.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{Connection, Hello, Mismatch, OpenAnswer, Request};
use waos_package::ProgramId;

/// What sending files to the WaOS on this machine came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Sent {
    /// The program has them.
    Opened,
    /// No WaOS listens here.
    NobodyAnswers,
}

/// Sends `files`, absolute paths, to the WaOS on this machine, to open with the program `id`, and
/// waits for its answer; fails with why it refused.
pub(crate) fn send(id: &ProgramId, files: Vec<PathBuf>) -> Result<Sent> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("a runtime to reach WaOS on")?;
    runtime.block_on(send_to(&waos_dev_protocol::listener_path(), id, files))
}

/// [`send`], to the WaOS listening at `socket`.
pub(crate) async fn send_to(socket: &Path, id: &ProgramId, files: Vec<PathBuf>) -> Result<Sent> {
    let mut connection = match Connection::connect(socket).await {
        Ok(connection) => connection,
        Err(error) if nobody_answers(&error) => return Ok(Sent::NobodyAnswers),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("connecting to WaOS at {}", socket.display()));
        }
    };
    let ours = Hello::waos_open(concat!("waos ", env!("CARGO_PKG_VERSION")));
    let theirs = connection.hello(&ours).await?;
    let open = Request::Open {
        id: id.clone(),
        files,
    };
    theirs
        .accepts(&ours, &open)
        .map_err(|mismatch: Mismatch| anyhow::anyhow!("{mismatch}"))?;
    connection.send(&open).await.context("sending to WaOS")?;
    match connection
        .receive::<OpenAnswer>()
        .await
        .context("reading from WaOS")?
    {
        Some(OpenAnswer::Opened) => Ok(Sent::Opened),
        Some(OpenAnswer::Refused(refusal)) => bail!("WaOS refused: {}", refusal.message),
        Some(OpenAnswer::Unknown) => {
            bail!("WaOS answered an opening in a way this build does not know")
        }
        None => bail!("WaOS closed the connection without an answer"),
    }
}

/// Whether `error` says no WaOS listens: no socket file, or one nobody answers on.
fn nobody_answers(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::NotFound | ErrorKind::ConnectionRefused
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nobody_answers_where_no_waos_listens() {
        let folder = tempfile::tempdir().unwrap();
        let id: ProgramId = "dev.example.notes".parse().unwrap();
        let sent = send_to(&folder.path().join("dev.sock"), &id, Vec::new()).await;
        assert_eq!(sent.unwrap(), Sent::NobodyAnswers);
    }
}
