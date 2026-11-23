//! The program's service extensions, served from Waft's table of them: each call the platform
//! carries from a development tool (`service-extensions`' `calls`) handed to the framework through
//! `EmbedderClient::service_extension_call`, and its answer handed back, the JSON as its text, as
//! the platform's `exit-request` is answered through `AppExitReply` (`app_exit`). Only a debug
//! build opens the stream (`events`), as Flutter's service extensions exist in debug and profile
//! builds alone; a release build serves none, and the platform answers its calls itself.
//!
//! The invariant: every call is answered once, through the reply the framework holds, which
//! answers `failed` when the framework drops it, so a tool never waits for a call no one holds.

use waft_embedder::{
    ServiceExtensionCall, ServiceExtensionError, ServiceExtensionParams, ServiceExtensionReply,
    ServiceExtensionResult,
};
use wasdk::service_extensions::{ExtensionCall, ExtensionError};

/// The framework's call for the platform's `call`, its reply answering the platform. Answering
/// it calls the platform, never the client, so it may be answered inside a call into the client.
pub(crate) fn framework_call(call: ExtensionCall) -> ServiceExtensionCall {
    let name = call.name();
    let params = ServiceExtensionParams::new(call.params());
    let reply = ServiceExtensionReply::new(move |result| {
        let answer = platform_answer(result);
        ExtensionCall::respond(call, answer.as_deref());
    });
    ServiceExtensionCall {
        name,
        params,
        reply,
    }
}

/// The framework's answer as the platform's: the JSON as its text, or the same error.
fn platform_answer(result: ServiceExtensionResult) -> Result<String, ExtensionError> {
    match result {
        Ok(json) => serde_json::to_string(&json)
            .map_err(|error| ExtensionError::Failed(format!("the answer as JSON: {error}"))),
        Err(ServiceExtensionError::Unknown { registered, .. }) => {
            Err(ExtensionError::Unknown(registered))
        }
        Err(ServiceExtensionError::InvalidParams(why)) => Err(ExtensionError::InvalidParams(why)),
        Err(ServiceExtensionError::Failed(why)) => Err(ExtensionError::Failed(why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_its_json_text_and_each_error_the_platforms() {
        let json = serde_json::json!({"tree": "Root", "depth": 2});
        assert_eq!(
            platform_answer(Ok(json.clone())).ok(),
            Some(serde_json::to_string(&json).unwrap())
        );
        let unknown = ServiceExtensionError::Unknown {
            name: "ext.waft.nothing".to_owned(),
            registered: vec!["ext.waft.debugDumpApp".to_owned()],
        };
        assert!(matches!(
            platform_answer(Err(unknown)),
            Err(ExtensionError::Unknown(registered)) if registered == ["ext.waft.debugDumpApp"]
        ));
        assert!(matches!(
            platform_answer(Err(ServiceExtensionError::InvalidParams("no id".into()))),
            Err(ExtensionError::InvalidParams(why)) if why == "no id"
        ));
        assert!(matches!(
            platform_answer(Err(ServiceExtensionError::Failed("broke".into()))),
            Err(ExtensionError::Failed(why)) if why == "broke"
        ));
    }
}
