//! What a program holding the inspector role sees of the others, which the embedder gives: every
//! program it knows, installed or running, the inspector among them (decision 12 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`), each change to them, and a
//! program's icon, asked for one at a time; and a program started held, which the embedder starts
//! as it starts any, or ended (decisions 18 and 19). What a running one does the engine reads itself, of
//! the program it runs under that id.
//!
//! The invariant: the programs are said in `wasdk:inspect`'s own types, so what the embedder gives
//! is what the inspector reads, with no second schema between them.

use futures::future::BoxFuture;
use futures::stream::BoxStream;

use super::bindings::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use crate::running::HeldProgram;

/// The programs on this device, as an inspector sees them. Each answer is one of the embedder's
/// own, read when asked; called from a program's thread.
pub trait InspectedPrograms: Send + Sync {
    /// This device's name, as its user knows it.
    fn device_name(&self) -> String;

    /// Every program installed, running, or run in development and ended with days kept of it,
    /// each once.
    fn programs(&self) -> BoxFuture<'static, Vec<Program>>;

    /// Each change to them from now on.
    fn program_changes(&self) -> BoxStream<'static, ProgramChange>;

    /// The icon of the program `id` names, its package's PNG; none for one without an icon.
    fn icon(&self, id: &str) -> BoxFuture<'static, Option<Vec<u8>>>;

    /// Starts the program `id` names with [`WaOS::run_held`](crate::WaOS::run_held), ending the
    /// one running first with a stop: installed, or run in development from the component it was
    /// last sent as. Answers once it is linked, so its trace can be listened to.
    fn start_held(&self, id: &str) -> BoxFuture<'static, Result<HeldProgram, StartError>>;

    /// Ends the program `id` names where it runs, with a stop; resolves once it has ended.
    fn end(&self, id: &str) -> BoxFuture<'static, ()>;
}
