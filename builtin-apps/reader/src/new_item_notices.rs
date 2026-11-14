//! Telling the user of new items with a notification, where they turned that on: after a refresh
//! the app made on its schedule found items it did not have, one notification says how many and
//! in which feeds, in place of the one before it. A click on it brings the window forward with
//! every feed's items listed, the click that launched the program among them.
//!
//! Turning it on asks for the notifications permission first, as `notifications` says, since a
//! post without it answers `denied` and never asks; it stays off where the user refuses.

use waft::App;
use waft::ui::{Notification, NotificationActivation, PermissionState};
use waft_winui::InfoBarSeverity;

use crate::feed::FeedId;
use crate::library::Scope;
use crate::reader_app::ReaderApp;
use crate::showing::list_items;
use crate::window::bring_forward;

/// The id every such notification is posted under, so that each replaces the last.
const NOTICE_ID: &str = "new-items";

const DENIED: &str = "Reader may not show notifications. Allow them for Reader in WaOS’s settings, and in the system’s.";
const NO_NOTIFICATIONS: &str = "This system shows no notifications.";

/// What a refresh found in one feed that the library did not have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) feed: FeedId,
    /// What the feed is called.
    pub(crate) title: String,
    pub(crate) count: usize,
    /// Whether this was the first time the feed was fetched, when everything it lists is new.
    pub(crate) first_fetch: bool,
}

/// Posts a notification of what a refresh `found`, where the user turned that on.
pub(crate) fn tell_of_new_items(app: &mut App, reader: &ReaderApp, found: &[Found]) {
    if !reader.settings.read(app).notify_of_new_items {
        return;
    }
    let notice = notice_of(found);
    let platform = app.platform();
    let Some(notifications) = platform.notifications() else {
        return;
    };
    let posting = notifications.post(&notice);
    app.spawn(async move |_cx| {
        if let Err(error) = posting.await {
            eprintln!("the notification of new items is not posted: {error:?}");
        }
    });
}

/// The user turned the notifications on or off: on, the permission is asked for first, and the
/// setting stays off, with why, unless it is granted.
pub(crate) fn set_notifying(app: &mut App, reader: &ReaderApp, on: bool) {
    if !on {
        return reader.change_settings(app, |settings| settings.notify_of_new_items = false);
    }
    let platform = app.platform();
    let Some(notifications) = platform.notifications() else {
        return reader.say(app, InfoBarSeverity::Warning, NO_NOTIFICATIONS);
    };
    let asking = notifications
        .permission()
        .map(|permission| permission.request());
    let reader = reader.clone();
    app.spawn(async move |cx| {
        let granted = match asking {
            Some(asking) => asking.await == PermissionState::Granted,
            None => true,
        };
        cx.update(|app| {
            if granted {
                reader.change_settings(app, |settings| settings.notify_of_new_items = true);
            } else {
                reader.say(app, InfoBarSeverity::Warning, DENIED);
            }
        });
    });
}

/// A click on the app's notification: the window comes forward, listing every feed's items.
pub(crate) fn notice_clicked(
    app: &mut App,
    reader: &ReaderApp,
    activation: &NotificationActivation,
) {
    if activation.id == NOTICE_ID {
        list_items(app, reader, Scope::All);
        bring_forward(app);
    }
}

/// The notification of what was `found`: how many items in its title, and the feeds beneath.
fn notice_of(found: &[Found]) -> Notification {
    let count: usize = found.iter().map(|found| found.count).sum();
    let title = if count == 1 {
        "1 new item".to_owned()
    } else {
        format!("{count} new items")
    };
    let feeds: Vec<&str> = found.iter().map(|found| found.title.as_str()).collect();
    let mut notice = Notification::new(NOTICE_ID, title);
    notice.body = Some(feeds.join(", "));
    notice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(title: &str, count: usize) -> Found {
        Found {
            feed: FeedId(1),
            title: title.to_owned(),
            count,
            first_fetch: false,
        }
    }

    #[test]
    fn a_notice_says_how_many_items_and_in_which_feeds() {
        let notice = notice_of(&[found("NASA", 2), found("Rust Blog", 1)]);
        assert_eq!(notice.id, NOTICE_ID);
        assert_eq!(notice.title, "3 new items");
        assert_eq!(notice.body.as_deref(), Some("NASA, Rust Blog"));
        assert_eq!(notice_of(&[found("NASA", 1)]).title, "1 new item");
    }
}
