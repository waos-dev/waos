//! `waos ext`: the service extensions of the programs running in the WaOS on this machine, as
//! Waft's CLI (`cargo waft ext`) reaches an app's, so WaOS is a platform to it and `waos` only
//! carries the messages (the design's decision 70). `waos ext list --json` lists each program
//! running that serves extensions as Waft's discovery lists an app, its address the command that
//! relays to it; `waos ext --id <id> --stdio` is that relay: it reads one message of Waft's
//! service protocol a line on stdin and writes one a line on stdout until stdin closes, each
//! request one `Extension` to WaOS, on a connection of its own as the command's other requests
//! are, and its answer one response.
//!
//! The invariants: every request read is answered with one response line, whatever WaOS says or
//! fails to, so a client never waits for one; and a hello is taken and not forwarded, since what
//! the relay trusts is WaOS's own socket, the user's alone, not a token.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use waft_service_protocol::{
    Address, EXTENSION_ERROR, INVALID_PARAMS, INVALID_REQUEST, ListedApp, METHOD_NOT_FOUND,
    Message, Request, Response, RpcError,
};
use waos_dev_protocol::{
    ExtensionAnswer, RefusalKind, Request as WaosRequest, ServingProgram, ServingPrograms,
    ServingProgramsAnswer,
};
use waos_package::ProgramId;

use crate::asking::{self, Unreached};

/// The device a listing names: the WaOS on this machine.
const DEVICE: &str = "WaOS";

/// Prints each program running that serves extensions: as Waft's listing, a JSON object a line,
/// with `json`, and a line of its name and id otherwise. With no WaOS answering, none runs.
pub async fn list(json: bool) -> Result<ExitCode> {
    let serving = match asking::ask_if_answered(&WaosRequest::ServingPrograms).await? {
        Unreached::Answered(ServingProgramsAnswer::Programs(serving)) => serving,
        Unreached::Answered(ServingProgramsAnswer::Refused(refusal)) => bail!("{refusal}"),
        Unreached::Answered(ServingProgramsAnswer::Unknown) => bail!(
            "WaOS answered the list in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`"
        ),
        Unreached::NobodyAnswers => ServingPrograms::default(),
    };
    for line in listing(&serving, json) {
        println!("{line}");
    }
    if !json && serving.programs.is_empty() {
        eprintln!("waos: no program running in WaOS serves service extensions");
    }
    Ok(ExitCode::SUCCESS)
}

/// The lines `list` prints of `serving`.
fn listing(serving: &ServingPrograms, json: bool) -> Vec<String> {
    serving
        .programs
        .iter()
        .map(|program| match json {
            true => listed(program, serving.pid),
            false => format!("{}  {}", program.name, program.id),
        })
        .collect()
}

/// `program`, in the WaOS of process `pid`, as one line of Waft's listing.
fn listed(program: &ServingProgram, pid: u32) -> String {
    listed_app(program, pid).to_json().to_string()
}

/// `program`, in the WaOS of process `pid`, as Waft's discovery lists an app: reached through the
/// relay, with no token, WaOS's socket being the user's alone.
fn listed_app(program: &ServingProgram, pid: u32) -> ListedApp {
    let id = program.id.as_str();
    let relay = ["waos", "ext", "--id", id, "--stdio"].map(String::from);
    ListedApp {
        app: program.name.clone(),
        identifier: Some(id.to_owned()),
        device: Some(DEVICE.to_owned()),
        pid,
        address: Address::Command(relay.to_vec()),
        token: String::new(),
    }
}

/// Relays between stdin and stdout and the program `id` in the WaOS on this machine, until stdin
/// closes.
pub async fn relay_stdio(id: ProgramId) -> Result<ExitCode> {
    let input = tokio::io::BufReader::new(tokio::io::stdin());
    relay(
        &id,
        &waos_dev_protocol::listener_path(),
        input,
        tokio::io::stdout(),
    )
    .await?;
    Ok(ExitCode::SUCCESS)
}

/// Relays the messages `input` holds, one a line, to the program `id` in the WaOS listening at
/// `socket`, writing each answer to `output` as a line, until `input` ends.
pub async fn relay(
    id: &ProgramId,
    socket: &Path,
    input: impl AsyncBufRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
) -> Result<()> {
    let mut lines = input.lines();
    while let Some(line) = lines.next_line().await.context("reading stdin")? {
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = answer_of(id, socket, &line).await else {
            continue;
        };
        let mut text = Message::Response(response).to_text();
        text.push('\n');
        output
            .write_all(text.as_bytes())
            .await
            .context("writing stdout")?;
        output.flush().await.context("writing stdout")?;
    }
    Ok(())
}

/// The response the message `line` holds is answered with; none for a hello, which is taken.
async fn answer_of(id: &ProgramId, socket: &Path, line: &str) -> Option<Response> {
    match Message::from_text(line) {
        Err(unreadable) => Some(unreadable),
        Ok(Message::Hello { .. }) => None,
        Ok(Message::Request(request)) => Some(called(id, socket, request).await),
        Ok(Message::Response(response)) => Some(Response::error(
            response.id,
            RpcError::new(
                INVALID_REQUEST,
                "a response is no request: the relay answers requests".to_owned(),
            ),
        )),
    }
}

/// `request` called of the program `id` as one `Extension`, and the response its answer is.
async fn called(id: &ProgramId, socket: &Path, request: Request) -> Response {
    let Request {
        id: call,
        method,
        params,
    } = request;
    let extension = WaosRequest::Extension {
        id: id.clone(),
        name: method.clone(),
        params: params.into_iter().collect(),
    };
    let answer = asking::connect_at(socket)
        .await
        .map(|connection| asking::answer::<ExtensionAnswer>(connection, &extension));
    let answer = match answer {
        Ok(answering) => answering.await,
        Err(error) => Err(error),
    };
    match answer {
        Ok(answer) => Response {
            id: call,
            outcome: outcome_of(answer),
        },
        Err(error) => Response::error(call, RpcError::new(EXTENSION_ERROR, format!("{error:#}"))),
    }
}

/// What WaOS's `answer` is as a response's outcome.
fn outcome_of(answer: ExtensionAnswer) -> Result<Value, RpcError> {
    match answer {
        ExtensionAnswer::Answered(json) => serde_json::from_str(&json).map_err(|error| {
            RpcError::new(EXTENSION_ERROR, format!("the program answered no JSON: {error}"))
        }),
        ExtensionAnswer::Refused(refusal) => Err(match refusal.kind {
            RefusalKind::UnknownExtension { registered } => {
                RpcError::new(METHOD_NOT_FOUND, refusal.message)
                    .with_data(json!({ "registered": registered }))
            }
            RefusalKind::InvalidParams => RpcError::new(INVALID_PARAMS, refusal.message),
            _ => RpcError::new(EXTENSION_ERROR, refusal.message),
        }),
        ExtensionAnswer::Unknown => Err(RpcError::new(
            EXTENSION_ERROR,
            "WaOS answered in a way this waos does not know: update waos with `curl -fsSL https://waos.app/install.sh | sh`".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{Hello, Listener, Refusal, TestPlace};

    use super::*;

    fn notes() -> ProgramId {
        "dev.example.notes.development".parse().unwrap()
    }

    #[test]
    fn a_program_is_listed_as_waft_lists_an_app_its_address_the_relay() {
        let serving = ServingPrograms {
            pid: 4242,
            programs: vec![ServingProgram {
                id: notes(),
                name: "Notes (Development)".to_owned(),
                running: true,
            }],
        };
        let [line] = listing(&serving, true).try_into().expect("one line");
        assert_eq!(
            ListedApp::from_json(&line),
            Some(listed_app(&serving.programs[0], 4242)),
            "the line Waft reads back"
        );
        assert_eq!(
            listed_app(&serving.programs[0], 4242).address.to_string(),
            r#"command:["waos","ext","--id","dev.example.notes.development","--stdio"]"#
        );
        assert_eq!(
            listing(&serving, false),
            ["Notes (Development)  dev.example.notes.development"]
        );
    }

    /// A WaOS of the test's own at `place`, answering each `Extension` it is asked with what
    /// `answer` makes of the extension's name and parameters, for `calls` connections.
    async fn fake_waos(
        place: &TestPlace,
        calls: usize,
        answer: impl Fn(&str, &[(String, String)]) -> ExtensionAnswer + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        let mut listener = Listener::bind(place.path()).await.unwrap();
        tokio::spawn(async move {
            for _ in 0..calls {
                let mut server = listener.accept().await.unwrap();
                server
                    .accept_hello(&Hello::waos("WaOS 0.6.0"))
                    .await
                    .unwrap();
                let Some(WaosRequest::Extension { id, name, params }) =
                    server.receive::<WaosRequest>().await.unwrap()
                else {
                    panic!("an extension's call");
                };
                assert_eq!(id, notes());
                server.send(&answer(&name, &params)).await.unwrap();
            }
        })
    }

    /// What the relay writes, a line each, for `input`.
    async fn relayed(place: &TestPlace, input: &str) -> Vec<Value> {
        let mut output = Vec::new();
        relay(&notes(), place.path(), input.as_bytes(), &mut output)
            .await
            .unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn each_request_is_one_extension_and_each_answer_one_response() {
        let place = TestPlace::new("ext-relay");
        let waos = fake_waos(&place, 3, |name, params| match name {
            "ext.waft.debugDumpApp" => {
                assert_eq!(params, [("depth".to_owned(), "2".to_owned())]);
                ExtensionAnswer::Answered(r#"{"data":"WidgetsBinding - DEBUG MODE"}"#.into())
            }
            "ext.waft.nothing" => ExtensionAnswer::Refused(Refusal::new(
                RefusalKind::UnknownExtension {
                    registered: vec!["ext.waft.debugDumpApp".into()],
                },
                "no ext.waft.nothing",
            )),
            _ => ExtensionAnswer::Refused(Refusal::new(RefusalKind::ExtensionFailed, "broke")),
        })
        .await;
        let input = [
            r#"{"token":"anything"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"ext.waft.debugDumpApp","params":{"depth":2}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ext.waft.nothing","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":"three","method":"ext.waft.broken"}"#,
            "not json",
        ]
        .join("\n");
        let responses = relayed(&place, &input).await;
        waos.await.unwrap();
        assert_eq!(
            responses.len(),
            4,
            "the hello is taken, and answered with nothing"
        );
        assert_eq!(
            responses[0],
            json!({"jsonrpc": "2.0", "id": 1, "result": {"data": "WidgetsBinding - DEBUG MODE"}})
        );
        assert_eq!(responses[1]["id"], 2);
        assert_eq!(responses[1]["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(
            responses[1]["error"]["data"],
            json!({"registered": ["ext.waft.debugDumpApp"]})
        );
        assert_eq!(responses[2]["id"], "three");
        assert_eq!(responses[2]["error"]["code"], EXTENSION_ERROR);
        assert_eq!(responses[2]["error"]["message"], "broke");
        assert_eq!(
            responses[3]["error"]["code"],
            waft_service_protocol::PARSE_ERROR
        );
    }

    #[tokio::test]
    async fn invalid_params_and_a_program_not_running_answer_their_codes() {
        let place = TestPlace::new("ext-relay-codes");
        let waos = fake_waos(&place, 2, |name, _| match name {
            "ext.waft.inspector.getProperties" => {
                ExtensionAnswer::Refused(Refusal::new(RefusalKind::InvalidParams, "no arg"))
            }
            _ => ExtensionAnswer::Refused(Refusal::new(RefusalKind::NotRunning, "not running")),
        })
        .await;
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"ext.waft.inspector.getProperties"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ext.waft.debugDumpApp"}"#,
        ]
        .join("\n");
        let responses = relayed(&place, &input).await;
        waos.await.unwrap();
        assert_eq!(responses[0]["error"]["code"], INVALID_PARAMS);
        assert_eq!(responses[1]["error"]["code"], EXTENSION_ERROR);
        assert_eq!(responses[1]["error"]["message"], "not running");
    }

    #[tokio::test]
    async fn with_no_waos_answering_each_request_answers_an_extension_error() {
        let place = TestPlace::new("ext-relay-nobody");
        let responses = relayed(
            &place,
            r#"{"jsonrpc":"2.0","id":7,"method":"ext.waft.debugDumpApp"}"#,
        )
        .await;
        assert_eq!(responses[0]["id"], 7);
        assert_eq!(responses[0]["error"]["code"], EXTENSION_ERROR);
    }
}
