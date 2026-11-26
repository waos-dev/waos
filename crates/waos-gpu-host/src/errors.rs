//! A device's errors, as wgpu routes them: what wgpu-core refuses goes to the innermost error
//! scope whose filter takes its kind, which keeps the first it catches, or else to the device's
//! uncaptured errors; a device lost reports no errors, its loss being `lost`'s to tell. The message
//! is wgpu's: the call that failed, the object's label, and the error's sources, one a line.
//!
//! The invariant: errors no scope caught are kept for `uncaptured-errors` only up to a bound, so a
//! program that never reads them, or reads them late, holds no more than that; each is also
//! written to the log, up to a bound per device, so a program that never reads them does not fail
//! silently.

use std::error::Error as StdError;
use std::fmt::Write as _;
use std::sync::Mutex;

use futures::channel::mpsc;
use wgpu_types::error::{ErrorType, WebGpuError};

use crate::wasdk::gpu::types::{Error, ErrorFilter};

/// How many uncaptured errors wait for the program to read them; one past these is dropped.
const UNCAPTURED_ERRORS_KEPT: usize = 64;

/// How many uncaptured errors one device writes to the log before it stops: enough to read the
/// first ones by, not so many that a device failing every frame floods it.
const UNCAPTURED_ERRORS_LOGGED: u32 = 100;

/// Where one device's errors go, shared by every object made on it.
pub(crate) struct ErrorSink(Mutex<Routing>);

struct Routing {
    scopes: Vec<Scope>,
    uncaptured: mpsc::Sender<Error>,
    /// The uncaptured errors' reading end, until the program takes it.
    unread: Option<mpsc::Receiver<Error>>,
    logged: u32,
}

struct Scope {
    filter: ErrorFilter,
    error: Option<Error>,
}

impl ErrorSink {
    pub(crate) fn new() -> ErrorSink {
        let (uncaptured, unread) = mpsc::channel(UNCAPTURED_ERRORS_KEPT);
        ErrorSink(Mutex::new(Routing {
            scopes: Vec::new(),
            uncaptured,
            unread: Some(unread),
            logged: 0,
        }))
    }

    pub(crate) fn push_scope(&self, filter: ErrorFilter) {
        self.0.lock().unwrap().scopes.push(Scope {
            filter,
            error: None,
        });
    }

    /// The innermost scope's first error; none for the outer `Option` when no scope is open.
    pub(crate) fn pop_scope(&self) -> Option<Option<Error>> {
        self.0.lock().unwrap().scopes.pop().map(|scope| scope.error)
    }

    /// The uncaptured errors from now on, and those kept so far; none once taken.
    pub(crate) fn take_uncaptured(&self) -> Option<mpsc::Receiver<Error>> {
        self.0.lock().unwrap().unread.take()
    }

    /// Reports `error`, where there is one, as wgpu-core gave it for the call `fn_ident` on the
    /// object labelled `label`.
    pub(crate) fn report<E>(&self, error: Option<E>, fn_ident: &'static str, label: Option<&str>)
    where
        E: WebGpuError + StdError + Send + Sync + 'static,
    {
        let Some(error) = error else {
            return;
        };
        let error = match error.webgpu_error_type() {
            ErrorType::OutOfMemory => Error::OutOfMemory,
            ErrorType::Validation => Error::Validation(described(error, fn_ident, label)),
            ErrorType::Internal => Error::Internal(described(error, fn_ident, label)),
            ErrorType::DeviceLost => return,
        };
        self.dispatch(error);
    }

    fn dispatch(&self, error: Error) {
        let mut routing = self.0.lock().unwrap();
        let filter = filter_of(&error);
        match routing
            .scopes
            .iter_mut()
            .rev()
            .find(|scope| scope.filter == filter)
        {
            Some(scope) => {
                scope.error.get_or_insert(error);
            }
            None => {
                routing.log(&error);
                // A program that dropped the stream, or let it fill, misses this one.
                let _ = routing.uncaptured.try_send(error);
            }
        }
    }
}

impl Routing {
    fn log(&mut self, error: &Error) {
        if self.logged == UNCAPTURED_ERRORS_LOGGED {
            return;
        }
        self.logged += 1;
        tracing::warn!("a program's uncaptured GPU error: {error:?}");
        if self.logged == UNCAPTURED_ERRORS_LOGGED {
            tracing::warn!("too many uncaptured GPU errors; no more are logged for this device");
        }
    }
}

fn filter_of(error: &Error) -> ErrorFilter {
    match error {
        Error::OutOfMemory => ErrorFilter::OutOfMemory,
        Error::Validation(_) => ErrorFilter::Validation,
        Error::Internal(_) => ErrorFilter::Internal,
    }
}

/// `error` as wgpu describes it: in the context of the call and the object, then each source on
/// a line of its own, indented a step deeper.
fn described<E: StdError + Send + Sync + 'static>(
    error: E,
    fn_ident: &'static str,
    label: Option<&str>,
) -> String {
    let error = wgpu_core::error::ContextError {
        fn_ident,
        source: Box::new(error),
        label: label.unwrap_or_default().to_owned(),
    };
    let mut output = String::new();
    write_tree(&mut output, 1, &error);
    format!("Validation Error\n\nCaused by:\n{output}")
}

fn write_tree(output: &mut String, level: usize, error: &(dyn StdError + 'static)) {
    let mut write = |error: &(dyn StdError + 'static)| {
        let _ = writeln!(output, "{}{}", " ".repeat(level * 2), error);
        if let Some(source) = error.source() {
            write_tree(output, level + 1, source);
        }
    };
    match error.downcast_ref::<wgpu_core::error::MultiError>() {
        Some(multi) => multi.errors().for_each(|error| write(error)),
        None => write(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation(message: &str) -> Error {
        Error::Validation(message.to_owned())
    }

    #[test]
    fn the_innermost_scope_of_the_kind_keeps_the_first_error() {
        let sink = ErrorSink::new();
        sink.push_scope(ErrorFilter::Validation);
        sink.push_scope(ErrorFilter::OutOfMemory);
        sink.dispatch(validation("first"));
        sink.dispatch(validation("second"));
        assert!(
            matches!(sink.pop_scope(), Some(None)),
            "not this scope's kind"
        );
        assert!(matches!(sink.pop_scope(), Some(Some(Error::Validation(m))) if m == "first"));
        assert!(sink.pop_scope().is_none(), "no scope is open");
    }

    #[test]
    fn an_error_no_scope_catches_is_uncaptured_in_order() {
        let sink = ErrorSink::new();
        sink.dispatch(validation("one"));
        sink.dispatch(Error::OutOfMemory);
        let mut uncaptured = sink
            .take_uncaptured()
            .expect("the first take has the stream");
        assert!(matches!(uncaptured.try_recv(), Ok(Error::Validation(m)) if m == "one"));
        assert!(matches!(uncaptured.try_recv(), Ok(Error::OutOfMemory)));
        assert!(sink.take_uncaptured().is_none(), "a later take has none");
    }
}
