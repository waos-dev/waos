//! What `#[wasdk::main]` and `#[wasdk::worker]` expand to. Not for programs to call: the module
//! is public only so the macros' expansions can reach it.
//!
//! The program's `wasi:cli/run` is Rust's standard library's: on `wasm32-wasip3` it calls the
//! `fn main` `#[wasdk::main]` writes, which runs the program on wit-bindgen's executor until the
//! program's own future ends ([`run_program`]), and ends the program there, as `std` ends a
//! process when its `main` returns, whatever it spawned still waiting.
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
pub use inventory::submit;

use crate::bindings::wasdk::platform::workers::WorkerError;
use crate::bindings::wit_stream;
use crate::{StreamReader, StreamWriter, block_on};

/// What a program's `main` may return: `()` or a `Result` whose error is printed.
pub trait Outcome {
    /// The code the program ends with: 0 where it went well, 1 where it failed, its error
    /// printed after the name of the program's package.
    fn exit_code(self, package_name: &str) -> i32;
}

impl Outcome for () {
    fn exit_code(self, _package_name: &str) -> i32 {
        0
    }
}

impl<E: Display> Outcome for Result<(), E> {
    fn exit_code(self, package_name: &str) -> i32 {
        match self {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{package_name}: {error}");
                1
            }
        }
    }
}

/// The program: `program`, run on wit-bindgen's executor until its future ends, then the end of
/// the program with its [`Outcome`]'s code, which the engine reads as `wasi:cli/run`'s answer.
pub fn run_program<F: Future<Output = O>, O: Outcome>(program: fn() -> F, package_name: &str) -> ! {
    block_on(async {
        let code = program().await.exit_code(package_name);
        std::process::exit(code)
    })
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
