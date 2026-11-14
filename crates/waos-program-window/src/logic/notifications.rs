//! A program's notifications in Waft's terms: a notification as the program builds it, the
//! engine's, as Waft's, under the program's own id and its own title, an action it answers by
//! typing made through the system's replies where the system takes them; and what the user did to
//! one, Waft's activation, as the program hears it.
//!
//! The invariant: nothing here says whose name a notification is posted under. A process that
//! posts several programs' under one name keeps their ids apart, and says whose each is, itself.

use waft::ui::{self, NotificationActivation, NotificationReplies};
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{Notification, NotificationAction};

use super::menu_entries::waft_image;

/// `notification` in Waft's terms, under the program's own id and titled as the program titled
/// it, its actions answered by typing made through `replies`, where the system takes replies.
pub fn waft_notification(
    notification: &Notification,
    replies: Option<&dyn NotificationReplies>,
) -> ui::Notification {
    ui::Notification {
        id: notification.id.clone(),
        title: notification.title.clone(),
        body: notification.body.clone(),
        image: notification.image.as_ref().map(waft_image),
        actions: notification
            .actions
            .iter()
            .map(|action| waft_action(action, replies))
            .collect(),
        silent: notification.silent,
    }
}

/// `action` in Waft's terms: a field to reply in where it asks for one and the system takes
/// replies, a button otherwise.
fn waft_action(
    action: &NotificationAction,
    replies: Option<&dyn NotificationReplies>,
) -> ui::NotificationAction {
    match (&action.reply_placeholder, replies) {
        (Some(placeholder), Some(replies)) => {
            replies.reply_action(&action.id, &action.label, placeholder)
        }
        _ => ui::NotificationAction::button(&action.id, &action.label),
    }
}

/// What the user did to a notification, Waft's `activation`, as the program hears it.
pub fn activation_of(activation: NotificationActivation) -> Activation {
    Activation {
        id: activation.id,
        action: activation.action,
        reply: activation.reply,
    }
}

#[cfg(test)]
mod tests {
    use waos::bindings::wasdk::platform::menus::Image;

    use super::*;

    struct Replies;

    impl NotificationReplies for Replies {}

    fn notification() -> Notification {
        Notification {
            id: "greeting".to_owned(),
            title: "Hello".to_owned(),
            body: Some("From a program".to_owned()),
            image: Some(Image {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
                template: false,
            }),
            actions: vec![
                NotificationAction {
                    id: "later".to_owned(),
                    label: "Later".to_owned(),
                    reply_placeholder: None,
                },
                NotificationAction {
                    id: "answer".to_owned(),
                    label: "Answer".to_owned(),
                    reply_placeholder: Some("Say something".to_owned()),
                },
            ],
            silent: true,
        }
    }

    #[test]
    fn a_notification_keeps_the_programs_id_and_title_and_its_reply_action() {
        let shown = waft_notification(&notification(), Some(&Replies));
        assert_eq!(shown.id, "greeting");
        assert_eq!(shown.title, "Hello");
        assert_eq!(shown.body.as_deref(), Some("From a program"));
        assert_eq!(shown.image.map(|image| image.width), Some(1));
        assert!(shown.silent);
        let placeholders: Vec<_> = shown
            .actions
            .iter()
            .map(|action| (action.id.as_str(), action.reply_placeholder()))
            .collect();
        assert_eq!(
            placeholders,
            [("later", None), ("answer", Some("Say something"))]
        );
    }

    #[test]
    fn a_reply_action_is_a_button_where_the_system_takes_no_replies() {
        let shown = waft_notification(&notification(), None);
        assert!(
            shown
                .actions
                .iter()
                .all(|action| action.reply_placeholder().is_none())
        );
    }

    #[test]
    fn a_click_is_heard_as_it_was_made() {
        let heard = activation_of(NotificationActivation {
            id: "greeting".to_owned(),
            action: Some("answer".to_owned()),
            reply: Some("hi".to_owned()),
        });
        assert_eq!(heard.id, "greeting");
        assert_eq!(heard.action.as_deref(), Some("answer"));
        assert_eq!(heard.reply.as_deref(), Some("hi"));
    }
}
