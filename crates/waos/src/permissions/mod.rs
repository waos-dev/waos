//! `wasdk:platform/permissions`: what the user lets a program use of what is guarded, read live
//! from the program's grants at each call, and asked of the user through them: whoever keeps the
//! grants asks a question once and keeps its answer, and opens a switch's page in the user's
//! settings, each told through the program's access (`access`). The system
//! may guard a thing with a permission of its own too, as macOS does each program's notifications
//! ([`ShellSystemPermissions`](crate::shell::ShellSystemPermissions)): a request for
//! notifications the user allowed asks the system after WaOS, and the permission reads denied
//! where the user granted it and the system does not, the user's answer kept
//! (`system_permissions`). The interface is served to a program by [`host`].
//!
//! The invariant: nothing here copies a grant or what the system says. Every state is read from
//! the grants and the shell when it is asked for, so what the user or the system changes holds
//! from the next call on; a stream of changes hears a permission's state only when it is another
//! than the one it told last, and is woken by what the user changes in WaOS and by a request made
//! here, never by what the user changes in the system's settings, which no system tells.

pub(crate) mod host;
mod system_permissions;

use std::future::{Future, pending};

use futures::future::{self, BoxFuture, FutureExt};
use futures::stream::{self, BoxStream, StreamExt};
use waos_capabilities::{
    Answer, AnswerFuture, ChangedFuture, FileSystemGrant, NetworkGrant, ProgramGrants, Question,
    Switch,
};

use crate::access::{Happened, ProgramAccess, Prompt};
use crate::bindings::wasdk::platform::permissions::State;
use crate::shell::{SystemPermission, SystemPermissionState};
pub(crate) use system_permissions::ProgramSystemPermissions;

/// One guarded capability: what a program's `permission` handle stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Switch(Switch),
    Question(Question),
}

/// One program's permissions: what decides where they stand, and what asks for them: its
/// grants, which ask its user, and the system's own permissions.
pub(crate) struct ProgramPermissions {
    decisions: Decisions,
}

impl ProgramPermissions {
    /// A program's permissions, from the grants `access` reads, which ask its user, and what the
    /// system says, asked of it through `system`.
    pub(crate) fn new(
        access: ProgramAccess,
        system: ProgramSystemPermissions,
    ) -> ProgramPermissions {
        ProgramPermissions {
            decisions: Decisions { access, system },
        }
    }

    /// Where `permission` stands now.
    pub(crate) fn state(&self, permission: Permission) -> State {
        self.decisions.state(permission)
    }

    /// What the program's user answered `question`, whatever the system says; none to ask.
    fn user_answer(&self, question: Question) -> Option<Answer> {
        self.decisions.access.grants().answer(question)
    }

    /// Asks the user for `permission`, and resolves to where it stands once the user is done:
    /// a question not answered yet is put to the user, and its answer kept; what the user allowed
    /// is asked of the system too, where it guards it and does not allow it yet; a switch's page is opened in the user's settings, and the state read once the user
    /// leaves it.
    pub(crate) fn request(
        &self,
        permission: Permission,
    ) -> impl Future<Output = State> + Send + 'static + use<> {
        let decisions = self.decisions.clone();
        let asked: BoxFuture<'static, ()> = match permission {
            Permission::Switch(switch) => self.open_settings(switch),
            Permission::Question(question) => self.answered(question),
        };
        async move {
            asked.await;
            decisions.state(permission)
        }
    }

    /// Where `permission` stands after each change the user makes in WaOS, and each answer of
    /// the system to a request made here, from now on.
    pub(crate) fn changes(&self, permission: Permission) -> BoxStream<'static, State> {
        let watch = Watch {
            last: self.state(permission),
            decisions: self.decisions.clone(),
            permission,
        };
        stream::unfold(watch, Watch::next_change).boxed()
    }

    /// Opens `switch`'s page in the user's settings, and resolves once the user leaves it.
    fn open_settings(&self, switch: Switch) -> BoxFuture<'static, ()> {
        self.decisions.access.tell(Happened::SettingsOpened(switch));
        self.decisions.access.grants().open_settings(switch)
    }

    /// Resolves once `question` is answered: by the user, put to them unless they answered it
    /// before, and where they allowed it, by the system too, where it guards the thing and does
    /// not allow it yet; the system is not asked where the user did not allow it. Where the
    /// question stands then is read from the grants and the system, by [`Decisions::state`].
    pub(crate) fn answered(&self, question: Question) -> BoxFuture<'static, ()> {
        let answered = self.user_answered(question);
        let system = self.decisions.system.clone();
        let guarded = system_permission(Permission::Question(question));
        async move {
            if answered.await == Some(Answer::Granted)
                && let Some(guarded) = guarded
            {
                system.request(guarded).await;
            }
        }
        .boxed()
    }

    /// The user's answer to `question`: the one kept, or else theirs once it is put to them; none
    /// where nobody answered.
    fn user_answered(&self, question: Question) -> BoxFuture<'static, Option<Answer>> {
        match self.user_answer(question) {
            None => self.ask(question).boxed(),
            answer => future::ready(answer).boxed(),
        }
    }

    /// Asks the user `question` through the grants, told as asked and answered: they ask it once
    /// however often the program asks, and keep the answer.
    fn ask(&self, question: Question) -> AnswerFuture {
        let access = &self.decisions.access;
        access.put_question(Prompt::Question(question), access.grants().ask(question))
    }
}

/// What decides where a program's permissions stand: its user, through its grants, and the
/// system, where it guards the thing too, both read live.
#[derive(Clone)]
struct Decisions {
    /// Where the grants are read: a permission's state is no use of what it guards.
    access: ProgramAccess,
    system: ProgramSystemPermissions,
}

impl Decisions {
    /// Where `permission` stands now: as the user set it, but denied where the user granted it and
    /// the system denies it.
    fn state(&self, permission: Permission) -> State {
        let state = state_of(self.access.grants(), permission);
        if state == State::Granted && self.denied_by_system(permission) {
            State::Denied
        } else {
            state
        }
    }

    fn denied_by_system(&self, permission: Permission) -> bool {
        system_permission(permission)
            .is_some_and(|guarded| self.system.state(guarded) == SystemPermissionState::Denied)
    }

    /// Resolves at the next change the user makes in WaOS, or answer of the system to a request
    /// made here, from now on.
    fn changed(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let granted = self.access.grants().changed();
        let answered = Box::pin(self.system.answered());
        future::select(granted, answered).map(drop)
    }
}

/// The system's permission that guards `permission` too, where a system has one.
fn system_permission(permission: Permission) -> Option<SystemPermission<'static>> {
    match permission {
        Permission::Question(Question::Notifications) => Some(SystemPermission::Notifications),
        Permission::Question(Question::Camera | Question::Microphone) | Permission::Switch(_) => {
            None
        }
    }
}

/// Where `permission` stands by `grants`: a switch on or off, a question answered or not.
pub(crate) fn state_of(grants: &dyn ProgramGrants, permission: Permission) -> State {
    let granted = match permission {
        Permission::Switch(Switch::Network) => grants.network().on,
        Permission::Switch(Switch::LocalNetwork) => grants.local_network(),
        Permission::Switch(Switch::ClipboardRead) => grants.clipboard_read(),
        Permission::Switch(Switch::ClipboardWrite) => grants.clipboard_write(),
        Permission::Question(question) => match grants.answer(question) {
            None => return State::NotAsked,
            Some(answer) => answer.allows(),
        },
    };
    if granted {
        State::Granted
    } else {
        State::Denied
    }
}

/// One permission's state as a stream of changes last told it.
struct Watch {
    last: State,
    decisions: Decisions,
    permission: Permission,
}

impl Watch {
    /// The permission's next state another than the last told. The next change is asked for
    /// before the state is read, so a change between the two is heard.
    async fn next_change(mut self) -> Option<(State, Watch)> {
        loop {
            let changed = self.decisions.changed();
            let now = self.decisions.state(self.permission);
            if now != self.last {
                self.last = now;
                return Some((now, self));
            }
            changed.await;
        }
    }
}

/// The grants of a program given none: nothing granted, nothing mounted, every question, link
/// scheme and role denied, and nothing that changes.
pub(crate) struct NothingGranted;

impl ProgramGrants for NothingGranted {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: waos_capabilities::DomainRule::Only(Vec::new()),
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn role(&self, _role: waos_capabilities::Role) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
