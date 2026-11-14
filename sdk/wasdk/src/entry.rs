//! What `#[wasdk::main]` and `#[wasdk::worker]` expand to. Not for programs to call: the module
//! is public only so the macros' expansions can reach it.
//!
//! Every `#[wasdk::worker]` registers a [`WorkerEntry`] with `inventory`, a static constructor
//! that `wasm-ld` runs from `__wasm_call_ctors`, which every export wit-bindgen generates calls
//! first; the one `worker-entry.run` the program's `#[wasdk::main]` exports finds the entry there
//! by name. The invariant: an entry is registered by the crate that defines its function, so a
//! worker's function is in the program's own crate, whose code is always linked.

use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;

pub use crate::bindings::export;
pub use crate::bindings::exports::wasdk::platform::worker_entry::Guest as WorkerGuest;
pub use crate::bindings::exports::wasi::cli0_3_0::run::Guest;
pub use inventory::submit;

use crate::bindings::wasdk::platform::workers::WorkerError;
use crate::bindings::wit_stream;
use crate::{StreamReader, StreamWriter};

/// What a program's `main` may return: `()` or a `Result` whose error is printed.
pub trait Outcome {
    /// The engine's `run` answers `result` with no payload: the error was printed here, after
    /// the name of the program's package.
    #[allow(clippy::result_unit_err)]
    fn into_result(self, package_name: &str) -> Result<(), ()>;
}

impl Outcome for () {
    fn into_result(self, _package_name: &str) -> Result<(), ()> {
        Ok(())
    }
}

impl<E: Display> Outcome for Result<(), E> {
    fn into_result(self, package_name: &str) -> Result<(), ()> {
        self.map_err(|error| eprintln!("{package_name}: {error}"))
    }
}

/// The Preview 2 command entry a `wasm32-wasip2` `bin` always has, and all a native build of a
/// program is. The engine calls `wasi:cli/run@0.3.0` instead; whatever lands here is not it.
pub fn not_the_entry() -> ! {
    eprintln!(
        "this is a wasdk program: it runs as a component, whose entry is wasi:cli/run@0.3.0, and this is not that entry"
    );
    std::process::exit(1)
}

/// A worker's function, started with its inbox and its outbox.
pub type WorkerFunction =
    fn(StreamReader<Vec<u8>>, StreamWriter<Vec<u8>>) -> Pin<Box<dyn Future<Output = ()>>>;

/// One `#[wasdk::worker]`: the entry it is started at, the function's name, and the function.
pub struct WorkerEntry {
    pub name: &'static str,
    pub function: WorkerFunction,
}

inventory::collect!(WorkerEntry);

/// `worker-entry.run`: the worker registered at `entry`, run on the instance's executor with
/// `inbox` and the outbox whose reading end is answered; `unknown-entry` where none is. The
/// function starts after a yield to the platform, which has the answer by then, so whatever the
/// function does, a trap among it, is the worker's work and not its start.
pub fn run_worker(
    entry: String,
    inbox: StreamReader<Vec<u8>>,
) -> Result<StreamReader<Vec<u8>>, WorkerError> {
    let Some(worker) = inventory::iter::<WorkerEntry>
        .into_iter()
        .find(|worker| worker.name == entry)
    else {
        return Err(WorkerError::UnknownEntry(entry));
    };
    let (outbox, messages) = wit_stream::new();
    crate::spawn(async move {
        crate::yield_async().await;
        (worker.function)(inbox, outbox).await;
    });
    Ok(messages)
}
