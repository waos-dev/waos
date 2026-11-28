//! WaOS on one system: the one place that says what WaOS does there. The engine shows apps
//! through the system's shell (showing flows down), and what the user does through the system,
//! opening an app from the Dock or the Start menu, comes up as one stream, which startup connects
//! to what opens apps, so the system and the library open apps in the same place (what the user
//! did flows up). A system's shell never opens apps itself.
//!
//! The invariant an editor could break: startup is the only code that chooses a system's shell, so
//! no other shared code names a system. Today only macOS has its shell (`state::macos_shell`); the
//! other systems' parts are still put together in `state` (the design's step 2).

use futures::channel::oneshot;
use futures::stream::BoxStream;
use waos::{RunningProgram, Shell};
use waos_package::ProgramId;

/// WaOS on one system: how it shows apps, lists them, and hears what the user does there.
pub(crate) trait SystemShell {
    /// The parts the engine shows apps through: the engine's `Shell`, unchanged.
    fn shell(&self) -> Shell;
    /// The apps the user opened through the system: the Dock, the Start menu, a phone's shortcut.
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen>;
}

/// An app the user opened through the system, and where what opening it answers goes: the run
/// opened, which the system then shows as the app the user opened, or why it could not be opened.
/// An open nobody answers is refused as one WaOS could not open.
pub(crate) struct UserOpen {
    pub(crate) app: ProgramId,
    pub(crate) opened: oneshot::Sender<Result<RunningProgram, String>>,
}
