//! A program's notification as the system shows it: its id is the program's own under the
//! program's id, `<program id>/<its id>`, which keeps two programs' same id apart and says which
//! program a click is for; and, where WaOS posts it under its own name, its title follows the
//! program's name, which tells the user whose it is. Waft's notification has no subtitle, so the
//! name goes before the title, where every system shows it. A notification posted as the program
//! itself, as Windows lets WaOS post an installed program's, keeps its own title, the system
//! showing the program's name above it. The rest is the notification in Waft's terms, as a
//! program's app posts it on macOS (`waos-program-window`'s `waft_notification`).
//!
//! The invariant: a system id names the program that posted it. A program's id has no `/`, so the
//! first one ends it, and the program's own id may hold any.

use waft::ui::{self, NotificationActivation, NotificationReplies};
use waos::bindings::wasdk::platform::notifications::Activation;
use waos_package::ProgramId;
use waos_program_window::{activation_of, waft_notification};

/// Whose name a notification shows: the program's own, posted as it, or WaOS's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Titled {
    /// Posted as the program, under its own name: its title as it is.
    Own,
    /// Posted as WaOS: its title after the program's name.
    AfterTheProgram,
}

/// What separates the program's id from its own in a system id.
const SEPARATOR: char = '/';

/// The system's id of the notification `id` of the program running as `program`.
pub(crate) fn system_id(program: &str, id: &str) -> String {
    format!("{program}{SEPARATOR}{id}")
}

/// The program a click on the system's notification is for, and the click as that program hears
/// it, by its own id; none for a notification of no program's.
pub(crate) fn program_activation(
    activation: NotificationActivation,
) -> Option<(ProgramId, Activation)> {
    let (program, id) = activation.id.split_once(SEPARATOR)?;
    let program = program.parse().ok()?;
    let id = id.to_owned();
    Some((
        program,
        activation_of(NotificationActivation { id, ..activation }),
    ))
}

/// `notification` of the program running as `program`, which its user knows as `name`, as the
/// system shows it: under its system id, titled as `titled` says, its actions answered by typing
/// made through `replies`, where the system takes replies.
pub(crate) fn system_notification(
    program: &str,
    name: &str,
    titled: Titled,
    notification: &waos::Notification,
    replies: Option<&dyn NotificationReplies>,
) -> ui::Notification {
    let own = waft_notification(notification, replies);
    let title = match titled {
        Titled::Own => own.title.clone(),
        Titled::AfterTheProgram => format!("{name}: {}", own.title),
    };
    ui::Notification {
        id: system_id(program, &own.id),
        title,
        ..own
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Replies;

    impl NotificationReplies for Replies {}

    fn notification() -> waos::Notification {
        waos::Notification {
            id: "greeting".to_owned(),
            title: "Hello".to_owned(),
            body: Some("From a program".to_owned()),
            image: None,
            actions: vec![waos::NotificationAction {
                id: "answer".to_owned(),
                label: "Answer".to_owned(),
                reply_placeholder: Some("Say something".to_owned()),
            }],
            silent: true,
        }
    }

    #[test]
    fn a_notification_is_shown_under_its_programs_id_titled_after_its_name() {
        let shown = system_notification(
            "dev.example.notes",
            "Notes",
            Titled::AfterTheProgram,
            &notification(),
            Some(&Replies),
        );
        assert_eq!(shown.id, "dev.example.notes/greeting");
        assert_eq!(shown.title, "Notes: Hello");
        assert_eq!(shown.body.as_deref(), Some("From a program"));
        assert!(shown.silent);
        assert_eq!(shown.actions[0].reply_placeholder(), Some("Say something"));
    }

    #[test]
    fn a_notification_posted_as_the_program_keeps_its_own_title() {
        let shown = system_notification(
            "dev.example.notes",
            "Notes",
            Titled::Own,
            &notification(),
            None,
        );
        assert_eq!(shown.id, "dev.example.notes/greeting");
        assert_eq!(shown.title, "Hello");
    }

    #[test]
    fn a_click_is_for_the_program_its_id_begins_with_by_its_own_id() {
        let click = |id: &str| NotificationActivation {
            id: id.to_owned(),
            action: Some("answer".to_owned()),
            reply: Some("hi".to_owned()),
        };
        let (program, heard) =
            program_activation(click("dev.example.notes/inbox/7")).expect("a program's");
        assert_eq!(program.as_str(), "dev.example.notes");
        assert_eq!(heard.id, "inbox/7", "its own id may hold a slash");
        assert_eq!(heard.action.as_deref(), Some("answer"));
        assert_eq!(heard.reply.as_deref(), Some("hi"));
        for id in ["no-slash", "Not An Id/greeting", "/greeting"] {
            assert!(program_activation(click(id)).is_none(), "{id}");
        }
    }
}
