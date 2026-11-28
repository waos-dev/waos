//! App management, as a shell reaches it: open an installed app, stop one, and which apps run and
//! which are opening. It is the future privileged interface of that name in Rust form, which the
//! app instances serve (`app_instances`); a system's shell opens what the user opens through it
//! (the Dock, a shortcut, the library's icon) by asking it, then brings the app forward itself.
//!
//! A shell is made before the engine, and the app instances after it, so startup makes a mailbox
//! first ([`mailbox`]), as it makes the package manager's stop mailbox: the shells hold its
//! [`AppManagementMailbox`], and the app instances read its other end as they are made, with the
//! watches of what runs and what opens, so neither has to exist before the other.
//!
//! The invariant: every open the mailbox takes is answered exactly once, by the app instances, or
//! refused once nobody reads it.

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use tokio::sync::watch;
use waos_package::ProgramId;

use super::app_instances::{OpenConfiguration, OpenError, OpenStage, Opened, RunningInstances};

/// The opens under way, by app, each with its stage.
pub(crate) type Opening = BTreeMap<ProgramId, OpenStage>;

/// App management as a shell uses it.
pub(crate) trait AppManagement: Send + Sync {
    /// Opens the installed app `app` as `configuration` asks; answers its instance once it has
    /// started, or the one running where the configuration leaves it, or why it did not open.
    fn open(
        self: Arc<Self>,
        app: ProgramId,
        configuration: OpenConfiguration,
    ) -> BoxFuture<'static, Result<Opened, OpenError>>;

    /// Stops the instance of `app`, where one runs, and answers once it has ended, with whether
    /// one ran.
    fn stop(self: Arc<Self>, app: ProgramId) -> BoxFuture<'static, bool>;

    /// The instances running now, and each change of them from now on.
    fn running(&self) -> watch::Receiver<RunningInstances>;

    /// The opens under way now, and each change of them from now on.
    fn opening(&self) -> watch::Receiver<Opening>;
}

/// The shells' end of the mailbox to app management.
pub(crate) struct AppManagementMailbox {
    calls: UnboundedSender<AppManagementCall>,
    running: watch::Receiver<RunningInstances>,
    opening: watch::Receiver<Opening>,
}

/// The app instances' end of the mailbox: the calls to answer, and the watches to write what runs
/// and what opens to.
pub(crate) struct AppManagementCalls {
    pub(crate) calls: UnboundedReceiver<AppManagementCall>,
    pub(crate) running: watch::Sender<RunningInstances>,
    pub(crate) opening: watch::Sender<Opening>,
}

/// One call a shell made through the mailbox, with where its answer goes.
pub(crate) enum AppManagementCall {
    Open {
        app: ProgramId,
        configuration: OpenConfiguration,
        answer: oneshot::Sender<Result<Opened, OpenError>>,
    },
    Stop {
        app: ProgramId,
        answer: oneshot::Sender<bool>,
    },
}

/// The mailbox to app management: the shells' end, and the app instances'.
pub(crate) fn mailbox() -> (Arc<AppManagementMailbox>, AppManagementCalls) {
    let (calls, received) = mpsc::unbounded();
    let (running, read_running) = watch::channel(RunningInstances::new());
    let (opening, read_opening) = watch::channel(Opening::new());
    let mailbox = AppManagementMailbox {
        calls,
        running: read_running,
        opening: read_opening,
    };
    let ends = AppManagementCalls {
        calls: received,
        running,
        opening,
    };
    (Arc::new(mailbox), ends)
}

impl AppManagementMailbox {
    /// Puts `call` in the mailbox, made with where its answer goes, and answers what it is
    /// answered; `unanswered` where nobody reads the mailbox, or drops the call.
    fn call<T: Send + 'static>(
        &self,
        call: impl FnOnce(oneshot::Sender<T>) -> AppManagementCall,
        unanswered: impl FnOnce() -> T + Send + 'static,
    ) -> BoxFuture<'static, T> {
        let (answer, answered) = oneshot::channel();
        // A call nobody reads comes back, and goes with where its answer goes.
        let _ = self.calls.unbounded_send(call(answer));
        answered
            .map(|answer| answer.unwrap_or_else(|_| unanswered()))
            .boxed()
    }
}

impl AppManagement for AppManagementMailbox {
    fn open(
        self: Arc<Self>,
        app: ProgramId,
        configuration: OpenConfiguration,
    ) -> BoxFuture<'static, Result<Opened, OpenError>> {
        let refused = app.clone();
        self.call(
            |answer| AppManagementCall::Open {
                app,
                configuration,
                answer,
            },
            move || {
                let why = anyhow::anyhow!("{refused} could not be opened: WaOS opens no apps now");
                Err(OpenError::Failed(why))
            },
        )
    }

    fn stop(self: Arc<Self>, app: ProgramId) -> BoxFuture<'static, bool> {
        // Nobody left to answer stops nothing, as nothing runs.
        self.call(|answer| AppManagementCall::Stop { app, answer }, || false)
    }

    fn running(&self) -> watch::Receiver<RunningInstances> {
        self.running.clone()
    }

    fn opening(&self) -> watch::Receiver<Opening> {
        self.opening.clone()
    }
}
