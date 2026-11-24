//! What the system says of one program's permissions, beside what its user allowed it in WaOS:
//! read from the shell ([`ShellSystemPermissions`]) each time it is asked, and asked of the system
//! once the user allowed the thing and the system does not allow it yet.
//!
//! The invariant: nothing here keeps what the system said. The shell alone holds it, so the user's
//! answer in WaOS and the system's stay two layers, neither written into the other; and what
//! waits for a change is woken only by a request made here, since no system tells an app that its
//! user changed a permission in the system's settings.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::watch;

use crate::running::RunningProgram;
use crate::shell::{ShellSystemPermissions, SystemPermission, SystemPermissionState};

/// One program's permissions with the system. Every clone asks the same shell for the same
/// program, and hears the same requests answered.
#[derive(Clone)]
pub(crate) struct ProgramSystemPermissions {
    /// None where the system guards nothing beyond what the user allowed in WaOS.
    shell: Option<Arc<dyn ShellSystemPermissions>>,
    program: RunningProgram,
    /// Told each time the system answered a request made here.
    answered: Arc<watch::Sender<()>>,
}

impl ProgramSystemPermissions {
    pub(crate) fn new(
        shell: Option<Arc<dyn ShellSystemPermissions>>,
        program: RunningProgram,
    ) -> ProgramSystemPermissions {
        ProgramSystemPermissions {
            shell,
            program,
            answered: Arc::new(watch::Sender::new(())),
        }
    }

    /// What the system says of `permission` now; allowed where it guards nothing.
    pub(crate) fn state(&self, permission: SystemPermission) -> SystemPermissionState {
        match &self.shell {
            Some(shell) => shell.state(permission, &self.program),
            None => SystemPermissionState::Allowed,
        }
    }

    /// Asks the system for `permission` in its own way, unless it allows it already, and resolves
    /// to what it says then.
    pub(crate) async fn request(self, permission: SystemPermission) -> SystemPermissionState {
        let Some(shell) = &self.shell else {
            return SystemPermissionState::Allowed;
        };
        if shell.state(permission, &self.program) == SystemPermissionState::Allowed {
            return SystemPermissionState::Allowed;
        }
        let state = shell.request(permission, &self.program).await;
        self.answered.send_replace(());
        state
    }

    /// Resolves once the system has answered a request made from now on.
    pub(crate) fn answered(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let (kept, mut answers) = (Arc::clone(&self.answered), self.answered.subscribe());
        async move {
            // Never an error: `kept` holds the sender while this waits.
            let _ = answers.changed().await;
            drop(kept);
        }
    }
}
