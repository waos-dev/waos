//! The code serving `wasdk:platform/notifications` to a program: a notification built with
//! setters, posted, scheduled or removed through the shell once the program may post, a reply
//! action added to the notification the engine keeps, and the activations of the program's
//! notifications, each a stream of its own.
//!
//! Every stream of activations holds the program thawed for the activation it delivered last,
//! until the program reads again or drops it (`stream_holding`).

use std::future::ready;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wasmtime::component::{
    Access, Accessor, HasData, Linker, Resource, ResourceTable, StreamReader,
};
use wasmtime_wasi::p3::bindings::clocks::system_clock::Instant;

use super::{
    Notification, NotificationAction, NotificationReplies, NotificationScheduling, Notifications,
    may_post,
};
use crate::access::ProgramAccess;
use crate::bindings::wasdk::platform::menus::Image;
use crate::bindings::wasdk::platform::notifications::{
    self, Activation, HostNotification, HostNotificationReplies, HostNotificationScheduling,
    HostNotificationSchedulingWithStore, HostNotifications, HostNotificationsWithStore,
    NotificationError,
};
use crate::running::RunningProgram;
use crate::shell::{NotificationFuture, ShellNotifications};
use crate::stream::stream_holding;

/// What the notifications interface reads and writes in a program's store.
pub(crate) struct NotificationsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a RunningProgram,
    /// The shell's notifications; none where the shell has none.
    pub(crate) notifications: Option<&'a Arc<dyn ShellNotifications>>,
    /// Where each post is decided, by the program's notifications question.
    pub(crate) access: &'a ProgramAccess,
}

pub(crate) trait NotificationsCtxView: Send {
    fn notifications_ctx(&mut self) -> NotificationsCtx<'_>;
}

struct HasNotifications;

impl HasData for HasNotifications {
    type Data<'a> = NotificationsCtx<'a>;
}

pub(crate) fn add_to_linker<T: NotificationsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    notifications::add_to_linker::<_, HasNotifications>(linker, T::notifications_ctx)
}

impl NotificationsCtx<'_> {
    /// What `send` asks of the shell for the program's `notification`, taken out of the table;
    /// `denied`, asking nothing, while the program may not post.
    fn send_unless_denied(
        &mut self,
        notification: Resource<Notification>,
        send: impl FnOnce(&RunningProgram, &Notification) -> NotificationFuture,
    ) -> wasmtime::Result<NotificationFuture> {
        let notification = self.table.delete(notification)?;
        if !may_post(self.access) {
            return Ok(Box::pin(ready(Err(NotificationError::Denied))));
        }
        Ok(send(self.program, &notification))
    }
}

impl notifications::Host for NotificationsCtx<'_> {
    fn get_notifications(&mut self) -> wasmtime::Result<Option<Resource<Notifications>>> {
        let place = self.notifications.cloned().map(Notifications);
        Ok(place
            .map(|notifications| self.table.push(notifications))
            .transpose()?)
    }
}

impl HostNotification for NotificationsCtx<'_> {
    fn new(&mut self, id: String, title: String) -> wasmtime::Result<Resource<Notification>> {
        Ok(self.table.push(Notification::new(id, title))?)
    }

    fn body(&mut self, notification: Resource<Notification>, body: String) -> wasmtime::Result<()> {
        self.table.get_mut(&notification)?.body = Some(body);
        Ok(())
    }

    fn image(
        &mut self,
        notification: Resource<Notification>,
        image: Image,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&notification)?.image = Some(image);
        Ok(())
    }

    fn add_action(
        &mut self,
        notification: Resource<Notification>,
        id: String,
        label: String,
    ) -> wasmtime::Result<()> {
        let actions = &mut self.table.get_mut(&notification)?.actions;
        actions.push(NotificationAction {
            id,
            label,
            reply_placeholder: None,
        });
        Ok(())
    }

    fn silent(&mut self, notification: Resource<Notification>) -> wasmtime::Result<()> {
        self.table.get_mut(&notification)?.silent = true;
        Ok(())
    }

    fn drop(&mut self, notification: Resource<Notification>) -> wasmtime::Result<()> {
        self.table.delete(notification)?;
        Ok(())
    }
}

impl HostNotificationReplies for NotificationsCtx<'_> {
    fn add_reply_action(
        &mut self,
        _replies: Resource<NotificationReplies>,
        notification: Resource<Notification>,
        id: String,
        label: String,
        placeholder: String,
    ) -> wasmtime::Result<()> {
        let actions = &mut self.table.get_mut(&notification)?.actions;
        actions.push(NotificationAction {
            id,
            label,
            reply_placeholder: Some(placeholder),
        });
        Ok(())
    }

    fn drop(&mut self, replies: Resource<NotificationReplies>) -> wasmtime::Result<()> {
        self.table.delete(replies)?;
        Ok(())
    }
}

impl HostNotificationScheduling for NotificationsCtx<'_> {
    fn drop(&mut self, scheduling: Resource<NotificationScheduling>) -> wasmtime::Result<()> {
        self.table.delete(scheduling)?;
        Ok(())
    }
}

impl<T: NotificationsCtxView + 'static> HostNotificationSchedulingWithStore<T>
    for HasNotifications
{
    async fn schedule(
        accessor: &Accessor<T, Self>,
        scheduling: Resource<NotificationScheduling>,
        notification: Resource<Notification>,
        at: Instant,
    ) -> wasmtime::Result<Result<(), NotificationError>> {
        let scheduled = accessor.with(|mut access| {
            let mut context = access.get();
            let shell = Arc::clone(&context.table.get(&scheduling)?.0);
            context.send_unless_denied(notification, |program, notification| {
                schedule(shell.as_ref(), program, notification, at)
            })
        })?;
        Ok(scheduled.await)
    }
}

/// `notification` of `program`'s scheduled through `shell` at `at`; failed where `at` is no time
/// the system can name, or the shell no longer schedules.
fn schedule(
    shell: &dyn ShellNotifications,
    program: &RunningProgram,
    notification: &Notification,
    at: Instant,
) -> NotificationFuture {
    let failed = |why: &str| -> NotificationFuture {
        Box::pin(ready(Err(NotificationError::Failed(why.to_owned()))))
    };
    let Some(at) = system_time_of(at) else {
        return failed("the time is out of the system's range");
    };
    match shell.scheduling() {
        Some(scheduling) => scheduling.schedule(program, notification, at),
        None => failed("the system schedules no notification"),
    }
}

/// The system's time `at` names, seconds and nanoseconds from the Unix epoch, either side of it;
/// none beyond the system's range.
fn system_time_of(at: Instant) -> Option<SystemTime> {
    let nanoseconds = Duration::from_nanos(u64::from(at.nanoseconds));
    let seconds = Duration::from_secs(at.seconds.unsigned_abs());
    let whole = if at.seconds >= 0 {
        UNIX_EPOCH.checked_add(seconds)?
    } else {
        UNIX_EPOCH.checked_sub(seconds)?
    };
    whole.checked_add(nanoseconds)
}

impl HostNotifications for NotificationsCtx<'_> {
    fn remove(
        &mut self,
        notifications: Resource<Notifications>,
        id: String,
    ) -> wasmtime::Result<()> {
        self.table.get(&notifications)?.0.remove(self.program, &id);
        Ok(())
    }

    fn replies(
        &mut self,
        notifications: Resource<Notifications>,
    ) -> wasmtime::Result<Option<Resource<NotificationReplies>>> {
        let offered = self.table.get(&notifications)?.0.offers_replies();
        let replies = offered.then_some(NotificationReplies);
        Ok(replies
            .map(|replies| self.table.push(replies))
            .transpose()?)
    }

    fn scheduling(
        &mut self,
        notifications: Resource<Notifications>,
    ) -> wasmtime::Result<Option<Resource<NotificationScheduling>>> {
        let shell = &self.table.get(&notifications)?.0;
        let scheduling = shell
            .scheduling()
            .is_some()
            .then(|| NotificationScheduling(Arc::clone(shell)));
        Ok(scheduling
            .map(|scheduling| self.table.push(scheduling))
            .transpose()?)
    }

    fn drop(&mut self, notifications: Resource<Notifications>) -> wasmtime::Result<()> {
        self.table.delete(notifications)?;
        Ok(())
    }
}

impl<T: NotificationsCtxView + 'static> HostNotificationsWithStore<T> for HasNotifications {
    /// Denied, asking nothing, while the program may not post.
    async fn post(
        accessor: &Accessor<T, Self>,
        notifications: Resource<Notifications>,
        notification: Resource<Notification>,
    ) -> wasmtime::Result<Result<(), NotificationError>> {
        let posted = accessor.with(|mut access| {
            let mut context = access.get();
            let shell = Arc::clone(&context.table.get(&notifications)?.0);
            context.send_unless_denied(notification, |program, notification| {
                shell.post(program, notification)
            })
        })?;
        Ok(posted.await)
    }

    fn activations(
        mut access: Access<T, Self>,
        _notifications: Resource<Notifications>,
    ) -> wasmtime::Result<StreamReader<Activation>> {
        let activations = access.get().program.notifications().activations();
        stream_holding(access, activations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_seconds_and_nanoseconds_either_side_of_the_epoch() {
        let after = Instant {
            seconds: 4_102_444_800,
            nanoseconds: 5,
        };
        assert_eq!(
            system_time_of(after),
            Some(UNIX_EPOCH + Duration::new(4_102_444_800, 5))
        );
        // A tenth of a microsecond, the finest time Windows keeps.
        let before = Instant {
            seconds: -1,
            nanoseconds: 999_999_900,
        };
        assert_eq!(
            system_time_of(before),
            Some(UNIX_EPOCH - Duration::from_nanos(100)),
            "a tenth of a microsecond before the epoch"
        );
    }
}
