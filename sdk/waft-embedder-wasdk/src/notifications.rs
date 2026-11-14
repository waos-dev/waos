//! Waft's notifications over `wasdk:platform/notifications`, one to one: each notification built on
//! the WIT's resource and posted, scheduled or removed by its id; the user's permission as the
//! `permissions` interface's `notifications` question; typed replies and delivery at a later time
//! where the platform has them. What the user does to a notification reaches the app as
//! `EmbedderClient::notification_activated`, from the stream the loop reads (`events`).
//!
//! A post is the WIT's, which answers `denied` without asking while the program's user has not
//! allowed it (decision 7 of WaOS's v1 interface), where Waft's native hosts ask first: an app asks
//! through the permission before it posts.

use std::future::ready;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use waft_embedder::{
    Notification, NotificationActivation, NotificationError, NotificationPermission,
    NotificationReplies, NotificationScheduling, Notifications, PermissionFuture, PermissionState,
    PostFuture,
};
use wasdk::notifications as platform;
use wasdk::permissions::{self, Permission, State};
use wasdk::system_clock::Instant;

use crate::menus::platform_image;

/// The platform's notifications, with what it offers beyond posting.
pub(crate) struct WasdkNotifications {
    notifications: Rc<platform::Notifications>,
    replies: Option<platform::NotificationReplies>,
    scheduling: Option<Rc<platform::NotificationScheduling>>,
    /// The program's permission to show them.
    permission: Permission,
}

impl WasdkNotifications {
    /// The platform's notifications, where it has them.
    pub(crate) fn get() -> Option<WasdkNotifications> {
        let notifications = platform::get_notifications()?;
        Some(WasdkNotifications {
            replies: notifications.replies(),
            scheduling: notifications.scheduling().map(Rc::new),
            notifications: Rc::new(notifications),
            permission: permissions::notifications(),
        })
    }

    /// `notification`, built on the WIT's resource, its actions answered by typing added through
    /// the platform's replies.
    fn platform_notification(&self, notification: &Notification) -> platform::Notification {
        let built = platform::Notification::new(&notification.id, &notification.title);
        if let Some(body) = &notification.body {
            built.body(body);
        }
        if let Some(image) = &notification.image {
            built.image(&platform_image(image));
        }
        for action in &notification.actions {
            match (action.reply_placeholder(), &self.replies) {
                (Some(placeholder), Some(replies)) => {
                    replies.add_reply_action(&built, &action.id, &action.label, placeholder);
                }
                _ => built.add_action(&action.id, &action.label),
            }
        }
        if notification.silent {
            built.silent();
        }
        built
    }
}

impl Notifications for WasdkNotifications {
    fn post(&self, notification: &Notification) -> PostFuture {
        let notifications = Rc::clone(&self.notifications);
        let built = self.platform_notification(notification);
        Box::pin(async move { notifications.post(built).await.map_err(framework_error) })
    }

    fn remove(&self, id: &str) {
        self.notifications.remove(id);
    }

    fn permission(&self) -> Option<&dyn NotificationPermission> {
        Some(self)
    }

    fn replies(&self) -> Option<&dyn NotificationReplies> {
        self.replies
            .as_ref()
            .map(|_| self as &dyn NotificationReplies)
    }

    fn scheduling(&self) -> Option<&dyn NotificationScheduling> {
        self.scheduling
            .as_ref()
            .map(|_| self as &dyn NotificationScheduling)
    }
}

/// The `permissions` interface's `notifications`, a question put to the program's user.
impl NotificationPermission for WasdkNotifications {
    fn state(&self) -> PermissionFuture {
        Box::pin(ready(framework_state(self.permission.state())))
    }

    fn request(&self) -> PermissionFuture {
        let asked = self.permission.request();
        Box::pin(async move { framework_state(asked.await) })
    }
}

/// The WIT's actions answered by typing, which Waft's reply actions become.
impl NotificationReplies for WasdkNotifications {}

impl NotificationScheduling for WasdkNotifications {
    fn schedule(&self, notification: &Notification, at: SystemTime) -> PostFuture {
        let Some(scheduling) = self.scheduling.clone() else {
            return failed("the platform schedules no notification");
        };
        let Some(at) = instant_of(at) else {
            return failed("the time is out of the platform's range");
        };
        let built = self.platform_notification(notification);
        Box::pin(async move {
            scheduling
                .schedule(built, at)
                .await
                .map_err(framework_error)
        })
    }
}

/// What the user did to a notification, as Waft's app hears it.
pub(crate) fn framework_activation(activation: platform::Activation) -> NotificationActivation {
    NotificationActivation {
        id: activation.id,
        action: activation.action,
        reply: activation.reply,
    }
}

fn failed(why: &str) -> PostFuture {
    Box::pin(ready(Err(NotificationError::Failed(why.to_owned()))))
}

fn framework_error(error: platform::NotificationError) -> NotificationError {
    match error {
        platform::NotificationError::Denied => NotificationError::Denied,
        platform::NotificationError::Failed(reason) => NotificationError::Failed(reason),
    }
}

fn framework_state(state: State) -> PermissionState {
    match state {
        State::NotAsked => PermissionState::NotAsked,
        State::Granted => PermissionState::Granted,
        State::Denied => PermissionState::Denied,
    }
}

/// `at` as the system clock's instant: seconds from the Unix epoch, either side of it, and the
/// nanoseconds after them; none beyond its range.
fn instant_of(at: SystemTime) -> Option<Instant> {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => Some(Instant {
            seconds: i64::try_from(after.as_secs()).ok()?,
            nanoseconds: after.subsec_nanos(),
        }),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).ok()?;
            Some(match before.subsec_nanos() {
                0 => Instant {
                    seconds: -whole,
                    nanoseconds: 0,
                },
                nanoseconds => Instant {
                    seconds: -whole - 1,
                    nanoseconds: 1_000_000_000 - nanoseconds,
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_time_is_seconds_and_nanoseconds_either_side_of_the_epoch() {
        // Tenths of a microsecond, the finest time Windows keeps.
        let after = instant_of(UNIX_EPOCH + Duration::new(4_102_444_800, 500)).unwrap();
        assert_eq!((after.seconds, after.nanoseconds), (4_102_444_800, 500));
        let before = instant_of(UNIX_EPOCH - Duration::from_nanos(100)).unwrap();
        assert_eq!(
            (before.seconds, before.nanoseconds),
            (-1, 999_999_900),
            "a tenth of a microsecond before the epoch"
        );
        let whole = instant_of(UNIX_EPOCH - Duration::from_secs(2)).unwrap();
        assert_eq!((whole.seconds, whole.nanoseconds), (-2, 0));
    }
}
