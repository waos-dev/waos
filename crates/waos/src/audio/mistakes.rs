//! A program's mistakes as web-audio-api reports them: by panicking, its message led by the
//! spec's name for the exception (`IndexSizeError - output port 3 is out of bounds`). Every call
//! from a program into the crate that may panic runs through here. Where the spec throws for the
//! program's own mistake, the panic becomes a trap carrying the crate's message, which ends the
//! program as an exception left uncaught ends a script ([`trapping`]); where the call may fail
//! whatever the program does, as a device does, it becomes the message the program is answered
//! with ([`failing`]).
//!
//! The invariant: what a panic caught here leaves locked or half done is the program's own graph,
//! which the trap leaves unused, or the device call that failed; nothing of the engine's is held
//! across a call into the crate.

use std::any::Any;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};

use futures::FutureExt;

/// `call`, a panic of web-audio-api's in it trapping the program with the crate's message.
pub(crate) fn trapping<T>(call: impl FnOnce() -> T) -> wasmtime::Result<T> {
    panic::catch_unwind(AssertUnwindSafe(call))
        .map_err(|panic| wasmtime::format_err!("{}", message_of(panic.as_ref())))
}

/// `call`, a panic of web-audio-api's in it answered as its message.
pub(crate) fn failing<T>(call: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(call)).map_err(|panic| message_of(panic.as_ref()))
}

/// `call` run to its end, a panic of web-audio-api's in it answered as its message.
pub(crate) async fn failing_async<T>(call: impl Future<Output = T>) -> Result<T, String> {
    AssertUnwindSafe(call)
        .catch_unwind()
        .await
        .map_err(|panic| message_of(panic.as_ref()))
}

/// What a panic said.
fn message_of(panic: &(dyn Any + Send)) -> String {
    match panic.downcast_ref::<&str>() {
        Some(message) => (*message).to_owned(),
        None => panic
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "web-audio-api failed".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_traps_with_the_crate_s_message() {
        let trap = trapping(|| panic!("IndexSizeError - output port 3 is out of bounds"))
            .expect_err("a panic traps");
        assert_eq!(
            trap.to_string(),
            "IndexSizeError - output port 3 is out of bounds"
        );
        assert_eq!(trapping(|| 7).unwrap(), 7);
    }

    #[test]
    fn a_panic_formatted_is_answered_as_its_message() {
        let port = 3;
        assert_eq!(
            failing(|| -> () { panic!("InvalidStateError - port {port}") }),
            Err("InvalidStateError - port 3".to_owned())
        );
    }
}
