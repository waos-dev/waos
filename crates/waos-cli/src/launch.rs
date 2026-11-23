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
use crate::connecting::{WaOSStarter, connect_or_start};

/// Launches the installed program `id`, `name` to its user, in the WaOS running or the one started
/// when none answers, with `arguments` and `variables` for this launch alone, and says so.
pub async fn launch(
    id: &ProgramId,
    name: &str,
    arguments: &[String],
    variables: &[(String, String)],
    starter: &dyn WaOSStarter,
) -> Result<()> {
    launch_on(
        connect_or_start(starter).await?,
        id,
        name,
        arguments,
        variables,
    )
    .await?;
    eprintln!(
        "waos: {name} is running in WaOS; `waos attach` reads what it writes, `waos stop` stops it"
    );
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{Hello, Listener, Refusal, RefusalKind, TestPlace};

    use super::*;

    /// Launches against a WaOS of the test's own at `place`, which answers with `answers`.
    async fn launched_with(place: &str, answers: Vec<LaunchAnswer>) -> Result<Launch> {
        let place = TestPlace::new(place);
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            server
                .accept_hello(&Hello::waos("WaOS 0.3.0"))
                .await
                .unwrap();
            let Some(Request::Launch(launch)) = server.receive::<Request>().await.unwrap() else {
                panic!("a launch");
            };
            for answer in answers {
                server.send(&answer).await.unwrap();
            }
            launch
        });
        let client = Connection::connect(&socket).await.unwrap();
        let id: ProgramId = "dev.example.notes.development".parse().unwrap();
        let arguments = ["--port".to_owned(), "8080".to_owned()];
        let variables = [("RUST_LOG".to_owned(), "debug".to_owned())];
        let launched = launch_on(client, &id, "notes", &arguments, &variables).await;
        let launch = served.await.unwrap();
        launched.map(|()| launch)
    }

    #[tokio::test]
    async fn a_launch_says_its_arguments_and_variables_and_returns_once_launched() {
        let launch = launched_with(
            "launch-1",
            vec![LaunchAnswer::Compiling, LaunchAnswer::Launched],
        )
        .await
        .unwrap();
        assert_eq!(launch.arguments, ["--port", "8080"]);
        assert_eq!(
            launch.variables,
            [("RUST_LOG".to_owned(), "debug".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_launch_refused_is_an_error_saying_why() {
        let refusal = Refusal::new(
            RefusalKind::NotInstalled,
            "dev.example.notes is not installed",
        );
        let error = launched_with("launch-2", vec![LaunchAnswer::Refused(refusal)])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not installed"), "{error}");
    }
}
