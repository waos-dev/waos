//! What `waos ext` asks (Waft's CLI through it): a call of a service
//! extension of any program running, however it was installed or started, carried to the program
//! through the engine (`RunningProgram::service_extensions`) and answered with its JSON text, or
//! refused with why: the program does not run, serves none, serves none of that name (with the
//! names it serves), takes no such parameters, or failed; and the programs running that serve
//! extensions, each with the name its user sees, for a tool's list of what it can call.
//!
//! The invariant: WaOS only carries the call; what an extension is and answers is the program's,
//! so nothing here reads the JSON it carries.

use waos::{ExtensionError, RunningProgram};
use waos_dev_protocol::{
    ExtensionAnswer, Refusal, RefusalKind, ServerConnection, ServingProgram, ServingPrograms,
    ServingProgramsAnswer,
};
use waos_package::ProgramId;
use waos_package_manager::PackageManager;

use super::dev_refusal;
use super::running_apps::RunningApps;

/// Answers a connection's `Extension`: the extension `name` of the program `id` called with
/// `params`, and its answer, or why there is none.
pub(crate) async fn call(
    mut connection: ServerConnection,
    runs: &RunningApps,
    id: &ProgramId,
    name: &str,
    params: Vec<(String, String)>,
) {
    let answer = match runs.running(id) {
        None => ExtensionAnswer::Refused(dev_refusal::not_running(id)),
        Some(program) if !program.service_extensions().serves() => {
            ExtensionAnswer::Refused(not_serving(id))
        }
        Some(program) => answered(
            id,
            name,
            program.service_extensions().call(name, params).await,
        ),
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// Answers a connection's `ServingPrograms`: each program running that serves extensions, by the
/// name `packages` gives it, in this WaOS's process.
pub(crate) async fn serving(
    mut connection: ServerConnection,
    runs: &RunningApps,
    packages: &PackageManager,
) {
    let programs = runs
        .programs()
        .await
        .iter()
        .filter(|program| program.service_extensions().serves())
        .filter_map(|program| serving_program(program, packages))
        .collect();
    let answer = ServingProgramsAnswer::Programs(ServingPrograms {
        pid: std::process::id(),
        programs,
    });
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// `program` as a tool lists it: its id, the name its user sees, its id's where it is installed
/// no longer.
fn serving_program(program: &RunningProgram, packages: &PackageManager) -> Option<ServingProgram> {
    let id: ProgramId = program.name().parse().ok()?;
    let name = packages
        .installed_program(&id)
        .map_or_else(|| id.to_string(), |installed| installed.name());
    Some(ServingProgram {
        id,
        name,
        running: true,
    })
}

/// The protocol's answer to the call of `name` of `id`, from what the program answered.
fn answered(id: &ProgramId, name: &str, answer: Result<String, ExtensionError>) -> ExtensionAnswer {
    let refusal = match answer {
        Ok(json) => return ExtensionAnswer::Answered(json),
        Err(ExtensionError::Unknown(registered)) if registered.is_empty() => not_serving(id),
        Err(ExtensionError::Unknown(registered)) => Refusal::new(
            RefusalKind::UnknownExtension { registered },
            format!("{id} serves no extension {name}"),
        ),
        Err(ExtensionError::InvalidParams(why)) => Refusal::new(
            RefusalKind::InvalidParams,
            format!("{name} of {id} takes no such parameters: {why}"),
        ),
        Err(ExtensionError::Failed(why)) => Refusal::new(
            RefusalKind::ExtensionFailed,
            format!("{name} of {id} failed: {why}"),
        ),
    };
    ExtensionAnswer::Refused(refusal)
}

/// `id` runs, but serves no extensions: it listens for none, as a release build does.
fn not_serving(id: &ProgramId) -> Refusal {
    Refusal::new(
        RefusalKind::NotServing,
        format!(
            "{id} serves no service extensions: a debug build of a Waft app serves Waft's, and a release build none"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes() -> ProgramId {
        "dev.example.notes.development".parse().unwrap()
    }

    #[test]
    fn each_answer_of_the_programs_is_the_protocols() {
        assert_eq!(
            answered(&notes(), "ext.waft.debugDumpApp", Ok("{}".into())),
            ExtensionAnswer::Answered("{}".into())
        );
        let kind = |answer: ExtensionAnswer| match answer {
            ExtensionAnswer::Refused(refusal) => refusal.kind,
            other => panic!("refused: {other:?}"),
        };
        let registered = vec!["ext.waft.debugDumpApp".to_owned()];
        assert_eq!(
            kind(answered(
                &notes(),
                "ext.waft.nothing",
                Err(ExtensionError::Unknown(registered.clone()))
            )),
            RefusalKind::UnknownExtension { registered }
        );
        assert_eq!(
            kind(answered(
                &notes(),
                "x",
                Err(ExtensionError::Unknown(Vec::new()))
            )),
            RefusalKind::NotServing,
            "a program whose stream went meanwhile"
        );
        assert_eq!(
            kind(answered(
                &notes(),
                "x",
                Err(ExtensionError::InvalidParams("no id".into()))
            )),
            RefusalKind::InvalidParams
        );
        assert_eq!(
            kind(answered(
                &notes(),
                "x",
                Err(ExtensionError::Failed("broke".into()))
            )),
            RefusalKind::ExtensionFailed
        );
    }

    /// Run with `builtin-apps/notes`, a Waft app, which draws where the engine makes hardware
    /// buffers.
    #[cfg(any(target_vendor = "apple", target_os = "linux"))]
    #[tokio::test]
    async fn a_waft_app_running_is_listed_and_its_extensions_answered_through_the_dev_socket() {
        use std::path::Path;
        use std::time::Instant;

        use waos_dev_protocol::Request;

        use crate::logic::dev_server::tests::{TestServer, development, started};
        use crate::logic::test_engine::WITHIN;

        const NOTES: &str = "dev.waos.notes";
        let server = TestServer::drawing("extensions");
        let mut client = server
            .run(NOTES, Path::new(waos_test_programs::NOTES))
            .await;
        started(&mut client).await;
        let id = development(NOTES);
        let deadline = Instant::now() + WITHIN;
        let listed = loop {
            let ServingProgramsAnswer::Programs(serving) =
                server.ask(&Request::ServingPrograms).await
            else {
                panic!("the programs serving extensions");
            };
            if !serving.programs.is_empty() {
                break serving;
            }
            assert!(
                Instant::now() < deadline,
                "the app never served its extensions"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert_eq!(listed.pid, std::process::id());
        assert_eq!(
            listed.programs,
            [ServingProgram {
                id: id.clone(),
                name: format!("{NOTES} (Development)"),
                running: true,
            }]
        );

        let call = |name: &str| Request::Extension {
            id: id.clone(),
            name: name.to_owned(),
            params: Vec::new(),
        };
        let ExtensionAnswer::Answered(json) = server.ask(&call("ext.waft.debugDumpApp")).await
        else {
            panic!("the app's tree");
        };
        let json: serde_json::Value = serde_json::from_str(&json).expect("JSON text");
        assert!(json.to_string().contains("WidgetsBinding"), "{json}");
        let ExtensionAnswer::Refused(refusal) = server.ask(&call("ext.waft.nothing")).await else {
            panic!("an extension it does not serve");
        };
        let RefusalKind::UnknownExtension { registered } = refusal.kind else {
            panic!("unknown, with the names served: {refusal:?}");
        };
        assert!(
            registered
                .iter()
                .any(|name| name == "ext.waft.debugDumpApp")
        );

        let not_running = Request::Extension {
            id: development("dev.waos.examples.forever"),
            name: "ext.waft.debugDumpApp".to_owned(),
            params: Vec::new(),
        };
        let ExtensionAnswer::Refused(refusal) = server.ask(&not_running).await else {
            panic!("refused");
        };
        assert_eq!(refusal.kind, RefusalKind::NotRunning);
    }

    #[tokio::test]
    async fn a_program_that_serves_none_is_refused_not_serving_and_not_listed() {
        use std::path::Path;

        use waos_dev_protocol::Request;

        use crate::logic::dev_server::tests::{TestServer, development, started};

        const FOREVER: &str = "dev.waos.examples.forever";
        let server = TestServer::start("extensions-none");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let call = Request::Extension {
            id: development(FOREVER),
            name: "ext.waft.debugDumpApp".to_owned(),
            params: Vec::new(),
        };
        let ExtensionAnswer::Refused(refusal) = server.ask(&call).await else {
            panic!("refused");
        };
        assert_eq!(refusal.kind, RefusalKind::NotServing);
        let ServingProgramsAnswer::Programs(serving) = server.ask(&Request::ServingPrograms).await
        else {
            panic!("the programs serving extensions");
        };
        assert!(serving.programs.is_empty(), "{serving:?}");
    }
}
