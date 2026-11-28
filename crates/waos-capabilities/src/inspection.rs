//! What an app holding the inspector role is served of the other apps on its device, by whoever
//! runs it: every app it knows, installed or running, the inspector among them, each change to
//! them, and an app's icon, asked for one at a time; an app started held, which it starts as it
//! starts any, or ended; and what an app that does not run sees of this computer's files. What a
//! running app does, and what it sees of the files, the engine reads itself, of the app it runs
//! under that id.
//!
//! The invariant: the apps are said in `wasdk:inspect`'s own types ([`crate::wit`]), so what is
//! served is what the inspector reads, with no second schema between them.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::BoxStream;

use crate::wit::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use crate::{ProgramFolders, ProgramGrants};

/// The apps on this device, as an inspector sees them. Each answer is the provider's own, read
/// when asked; called from the inspector's thread.
pub trait InspectedApps: Send + Sync {
    /// This device's name, as its user knows it.
    fn device_name(&self) -> String;

    /// Every app installed, running, or run in development and ended with days kept of it, each
    /// once.
    fn apps(&self) -> BoxFuture<'static, Vec<Program>>;

    /// Each change to them from now on.
    fn app_changes(&self) -> BoxStream<'static, ProgramChange>;

    /// The icon of the app `id` names, its package's PNG; none for one without an icon.
    fn icon(&self, id: &str) -> BoxFuture<'static, Option<Vec<u8>>>;

    /// Starts the app `id` names held before its first instruction, ending the one running first
    /// with a stop: installed, or run in development from the component it was last sent as.
    /// Answers once it is linked, so its trace can be listened to, with what holds it.
    fn start_held(&self, id: &str) -> BoxFuture<'static, Result<AppHold, StartError>>;

    /// Ends the app `id` names where it runs, with a stop; resolves once it has ended.
    fn end(&self, id: &str) -> BoxFuture<'static, ()>;

    /// What the app `id` names sees of this computer's files while it does not run: its own
    /// folders and its grants; none for an app this device does not know.
    fn files(&self, id: &str) -> Option<AppFiles>;
}

/// What holds an app started held before its first instruction: dropping it lets the app go on.
pub struct AppHold {
    _held: Box<dyn Send>,
}

impl AppHold {
    /// What holds an app as `held` does, which lets the app go on once dropped.
    pub fn new(held: impl Send + 'static) -> AppHold {
        AppHold {
            _held: Box::new(held),
        }
    }
}

/// What an app that does not run sees of this computer's files: its own folders, where it has
/// them, and what its user lets it use, nothing where there are no grants.
#[derive(Clone)]
pub struct AppFiles {
    pub folders: Option<ProgramFolders>,
    pub grants: Option<Arc<dyn ProgramGrants>>,
}
