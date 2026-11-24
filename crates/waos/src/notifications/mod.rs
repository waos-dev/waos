//! `wasdk:platform/notifications`: the system's notifications, each a value posted under an id of
//! the program's own, replaced by a post of the same id.
//!
//! The shell posts, removes and schedules ([`ShellNotifications`]), told the program each is of,
//! so it keeps two programs' same id apart. The engine keeps the rest: the notification as the
//! program builds it ([`Notification`]), whether the program may post at all (the `permissions`
//! interface's `notifications` question, read from its grants at each post, decided and told
//! through its access: [`may_post`]), and what the user
//! does to its notifications ([`ProgramNotifications`]), news kept until heard, so the activation
//! that started a program waits for it to listen. Where the system keeps a permission of its own
//! for each program, as macOS does for each program's app, the permission reads `denied` too while
//! the system denies it, as the shell says each time it is read (`ShellSystemPermissions`). The
//! interface is served to a program by `host`.
//!
//! The invariant: nothing reaches the shell's `post` or `schedule` for a program its user has not
//! allowed notifications, and nothing asks the user on a post: a post without permission answers
//! `denied`. A post the system refuses answers `denied` too, and says nothing to the permission:
//! what the system says of the permission is the shell's to say, never read from an answer. Posts
//! still reach the shell while the system refuses, since it is the system that says whether it
//! takes them.

pub(crate) mod host;

use std::sync::Arc;

use futures::channel::mpsc;
use waos_capabilities::{Answer, ProgramGrants, Question};

use crate::access::{Asked, ProgramAccess, Refusal};
use crate::bindings::wasdk::platform::menus::Image;
use crate::bindings::wasdk::platform::notifications::Activation;
use crate::lifecycle::Lifecycle;
use crate::news::{KeptNews, Thawing};
use crate::shell::ShellNotifications;

/// Whether the program `access` is of may post a notification now: whether its user allowed it
/// notifications, whatever the system says, since a post the system refuses is the system's to
/// answer.
pub(crate) fn may_post(access: &ProgramAccess) -> bool {
    let user_allowed = |grants: &dyn ProgramGrants| {
        if grants.answer(Question::Notifications) == Some(Answer::Granted) {
            Ok(())
        } else {
            Err(Refusal::by("notifications"))
        }
    };
    access.decide(Asked::PostNotification, user_allowed).is_ok()
}

/// `notifications.notification`: one notification, as the program builds it and the shell posts
/// it.
#[derive(Clone, Debug)]
pub struct Notification {
    /// The program's own id for it.
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub image: Option<Image>,
    /// Its actions, in the order added.
    pub actions: Vec<NotificationAction>,
    /// Whether it is shown without the system's sound.
    pub silent: bool,
}

impl Notification {
    /// A notification under `id`, titled `title`, with nothing else set.
    fn new(id: String, title: String) -> Notification {
        Notification {
            id,
            title,
            body: None,
            image: None,
            actions: Vec::new(),
            silent: false,
        }
    }
}

/// One action of a notification: a button, or a field the user answers by typing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    /// What the action answers when chosen.
    pub id: String,
    pub label: String,
    /// For an action answered by typing, what its empty field shows; none for a button.
    pub reply_placeholder: Option<String>,
}

/// `notifications.notifications`: the shell's notifications, as the program reaches them.
pub struct Notifications(Arc<dyn ShellNotifications>);

/// `notifications.notification-replies`: the system's actions answered by typing, which add to a
/// notification the engine keeps, and need nothing of the shell.
pub struct NotificationReplies;

/// `notifications.notification-scheduling`: the shell's delivery at a later time.
pub struct NotificationScheduling(Arc<dyn ShellNotifications>);

/// What the user does to a program's notifications, as the shell reports it.
pub(crate) struct ProgramNotifications {
    /// What the program's thaw for an activation is owed to.
    lifecycle: Lifecycle,
    activations: KeptNews<Activation>,
}

impl ProgramNotifications {
    /// No activation yet, for the program whose lifecycle is `lifecycle`.
    pub(crate) fn new(lifecycle: Lifecycle) -> ProgramNotifications {
        ProgramNotifications {
            lifecycle,
            activations: KeptNews::default(),
        }
    }

    /// The user did `activation` to one of the program's notifications: every stream of its
    /// activations hears it, or, while none is open, the next one the program opens; the
    /// program thawed until it has read it.
    pub(crate) fn activated(&self, activation: Activation) {
        self.activations.tell(activation, &self.lifecycle);
    }

    /// The activations from now on, on a stream of their own, after those that waited for one.
    fn activations(&self) -> mpsc::UnboundedReceiver<Thawing<Activation>> {
        self.activations.subscribe(&self.lifecycle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::NothingGranted;

    #[test]
    fn a_post_the_user_did_not_allow_is_told_refused_by_the_notifications_question() {
        let access = ProgramAccess::new(Arc::new(NothingGranted));
        assert!(!may_post(&access));
        assert_eq!(access.refusals(), ["notifications"]);
    }
}
