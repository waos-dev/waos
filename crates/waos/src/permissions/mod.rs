//! `wasdk:platform/permissions`: what the user lets a program use of what is guarded, read live
//! from the program's grants at each call, and asked of the user through the shell's
//! [`Prompts`]: a question is put once, its answer kept through the grants, and a switch's page
//! opened in the user's settings, each told through the program's access (`access`). Where the
//! system keeps a permission of its own for each program, as macOS does for each program's app, it
//! may refuse the program its notifications too
//! (`SystemRefusal`): a request for notifications the user allowed puts the system's question after
//! WaOS's, through the shell's [`ShellSystemPermission`](crate::shell::ShellSystemPermission),
//! and the shell reports what else it learns of it; the permission then reads denied where the
//! user granted it, the user's answer kept. The interface is served to a program by [`host`].
//!
//! The invariant: nothing here copies a grant. Every state is read from the grants and the
//! system's refusal when it is asked for, so what the user or the system changes holds from the
//! next call on; a stream of changes hears a permission's state only when it is another than the
//! one it told last.

pub(crate) mod host;
mod system_refusal;

use std::future::{Future, pending};
use std::sync::Arc;

use futures::future::{self, BoxFuture, FutureExt};
use futures::stream::{self, BoxStream, StreamExt};
use waos_capabilities::{
    Answer, ChangedFuture, FileSystemGrant, NetworkGrant, ProgramGrants, Question, Switch,
};

use crate::access::{Happened, ProgramAccess, Prompt};
use crate::asking::{Asked, Asking};
use crate::bindings::wasdk::platform::permissions::State;
use crate::running::RunningProgram;
use crate::shell::{Prompts, ShellNotifications};
pub(crate) use system_refusal::SystemRefusal;

/// One guarded capability: what a program's `permission` handle stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Switch(Switch),
    Question(Question),
}

/// One program's permissions: what decides where they stand, and what asks for them: the shell's
/// prompts, which ask its user, and the system's own permission for its notifications.
pub(crate) struct ProgramPermissions {
    decisions: Decisions,
    prompts: Arc<dyn Prompts>,
    /// The system's notifications, where the shell has them, whose own permission for the program
    /// a request for notifications asks after its user, where the system keeps one.
    notifications: Option<Arc<dyn ShellNotifications>>,
    program: RunningProgram,
    /// The questions being put to the user now, each asked once however often the program asks.
    asking: Asking<Question>,
}

impl ProgramPermissions {
    /// `program`'s permissions, from the grants `access` reads and what the system says of its
    /// notifications, asked of its user through `prompts` and of the system through
    /// `notifications`.
    pub(crate) fn new(
        access: ProgramAccess,
        prompts: Arc<dyn Prompts>,
        notifications: Option<Arc<dyn ShellNotifications>>,
        program: RunningProgram,
    ) -> ProgramPermissions {
        ProgramPermissions {
            decisions: Decisions {
                access,
                notifications_refused: program.notifications_refused().clone(),
            },
            prompts,
            notifications,
            program,
            asking: Asking::default(),
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
    /// a question not answered yet is put to the user, and its answer kept; notifications the
    /// user allowed are asked of the system too, where it keeps a permission of its own for the
    /// program; a switch's page is opened in the user's settings, and the state read once the user
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

    /// Where `permission` stands after each change the user or the system makes from now on.
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
        self.prompts.open_settings(&self.program, switch).boxed()
    }

    /// Resolves once `question` is answered: by the user, put to them unless they answered it
    /// before, and for notifications they allowed, by the system too, where it keeps a permission
    /// of its own for the program; the system is not asked where the user did not allow them.
    /// Where the question stands then is read from the grants, by [`state_of`].
    pub(crate) fn answered(&self, question: Question) -> BoxFuture<'static, ()> {
        let answered = self.user_answered(question);
        let system = match question {
            Question::Notifications => self.system_question(),
            Question::Camera | Question::Microphone => None,
        };
        async move {
            if answered.await == Some(Answer::Granted)
                && let Some(system) = system
            {
                system.put().await;
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

    /// The system's own question for the program's notifications, where the shell has
    /// notifications; whether the system keeps a permission for the program is the shell's to say
    /// as it is put.
    fn system_question(&self) -> Option<SystemQuestion> {
        let notifications = Arc::clone(self.notifications.as_ref()?);
        Some(SystemQuestion {
            notifications,
            program: self.program.clone(),
            refusal: self.decisions.notifications_refused.clone(),
        })
    }

    /// Puts `question` to the user, unless it is being put already, and resolves to the answer,
    /// kept through the grants once it comes.
    fn ask(&self, question: Question) -> Asked {
        let access = &self.decisions.access;
        let kept = access.clone();
        self.asking.ask(
            question,
            || {
                access.put_question(
                    Prompt::Question(question),
                    self.prompts.ask(&self.program, question),
                )
            },
            move |answer| kept.grants().set_answer(question, answer),
        )
    }
}

/// The system's own question for a program's notifications, put once its user allowed them in
/// WaOS.
struct SystemQuestion {
    notifications: Arc<dyn ShellNotifications>,
    program: RunningProgram,
    /// Where the system's answer is kept.
    refusal: SystemRefusal,
}

impl SystemQuestion {
    /// Puts the question, where the system keeps a permission of its own for the program, and
    /// keeps whether it refuses the program its notifications once it answers; where it could not
    /// be asked, what it said before stands.
    async fn put(self) {
        let Some(permission) = self.notifications.system_permission() else {
            return;
        };
        if let Some(refused) = permission.request(&self.program).await {
            self.refusal.set(refused);
        }
    }
}

/// What decides where a program's permissions stand: its user, through its grants, read live, and
/// for its notifications the system too, which may refuse them whatever the user granted.
#[derive(Clone)]
struct Decisions {
    /// Where the grants are read: a permission's state is no use of what it guards.
    access: ProgramAccess,
    notifications_refused: SystemRefusal,
}

impl Decisions {
    /// Where `permission` stands now: as the user set it, but denied where the user granted it and
    /// the system refuses it.
    fn state(&self, permission: Permission) -> State {
        let state = state_of(self.access.grants(), permission);
        if state == State::Granted && self.refused_by_system(permission) {
            State::Denied
        } else {
            state
        }
    }

    fn refused_by_system(&self, permission: Permission) -> bool {
        permission == Permission::Question(Question::Notifications)
            && self.notifications_refused.refuses()
    }

    /// Resolves at the next change the user or the system makes from now on.
    fn changed(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let granted = self.access.grants().changed();
        let refused = Box::pin(self.notifications_refused.changed());
        future::select(granted, refused).map(drop)
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

/// The grants of a program given none: nothing granted, nothing mounted, every question and link
/// scheme denied, and nothing that changes.
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

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grants whose user allowed notifications, has not been asked for the camera, and allowed
    /// nothing else.
    struct NotificationsAllowed;

    impl ProgramGrants for NotificationsAllowed {
        fn network(&self) -> NetworkGrant {
            NothingGranted.network()
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

        fn answer(&self, question: Question) -> Option<Answer> {
            match question {
                Question::Notifications => Some(Answer::Granted),
                Question::Camera => None,
                Question::Microphone => Some(Answer::Denied),
            }
        }

        fn set_answer(&self, _question: Question, _answer: Answer) {}

        fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
            None
        }

        fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

        fn changed(&self) -> ChangedFuture {
            Box::pin(pending())
        }
    }

    fn decisions() -> Decisions {
        Decisions {
            access: ProgramAccess::new(Arc::new(NotificationsAllowed)),
            notifications_refused: SystemRefusal::default(),
        }
    }

    const NOTIFICATIONS: Permission = Permission::Question(Question::Notifications);

    #[test]
    fn notifications_the_user_allowed_read_denied_while_the_system_refuses_them() {
        let decisions = decisions();
        assert_eq!(decisions.state(NOTIFICATIONS), State::Granted);
        decisions.notifications_refused.set(true);
        assert_eq!(decisions.state(NOTIFICATIONS), State::Denied);
        assert_eq!(
            decisions.state(Permission::Question(Question::Camera)),
            State::NotAsked,
            "the system refuses the program nothing else"
        );
        assert_eq!(
            decisions.access.grants().answer(Question::Notifications),
            Some(Answer::Granted),
            "the user's answer is kept"
        );
        decisions.notifications_refused.set(false);
        assert_eq!(decisions.state(NOTIFICATIONS), State::Granted);
    }

    #[test]
    fn a_stream_of_changes_hears_the_system_refuse_and_let_again() {
        let decisions = decisions();
        let watch = Watch {
            last: State::Granted,
            decisions: decisions.clone(),
            permission: NOTIFICATIONS,
        };
        let mut changes = stream::unfold(watch, Watch::next_change).boxed();
        assert!(changes.next().now_or_never().is_none(), "nothing changed");
        decisions.notifications_refused.set(true);
        assert_eq!(changes.next().now_or_never(), Some(Some(State::Denied)));
        decisions.notifications_refused.set(true);
        assert!(
            changes.next().now_or_never().is_none(),
            "the system said the same again"
        );
        decisions.notifications_refused.set(false);
        assert_eq!(changes.next().now_or_never(), Some(Some(State::Granted)));
    }
}
