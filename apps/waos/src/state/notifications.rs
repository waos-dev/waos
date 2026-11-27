//! The programs' notifications on the main thread, over Waft's, where a program has no app of its
//! own (Windows, Linux and Android; on macOS its app posts them, `logic::program_apps`): each
//! program's posted, scheduled and removed as the system shows it (`logic::system_notifications`),
//! under its id inside the program's; and the system's own permission, WaOS's, asked before the
//! first notification any program posts or schedules. On Windows an installed program's are posted
//! as the program, under its identity (`logic::program_identities`), which its Start menu shortcut
//! makes known to the system, with its name and icon, and a click on one comes back here; every
//! other program's are WaOS's, titled after the program's name as its user knows it, as they are
//! everywhere else. GNOME files a notification by its sender's process, whatever app it names, so
//! on Linux a program's are WaOS's. Whether a program may post at all is its own permission, which
//! the engine keeps. The notifications are the engine's shell's only where Waft's platform has
//! them.
//!
//! The invariant: the system is asked for WaOS's permission once, before anything is shown, since
//! the system's post never asks and is refused until the user allows; every post and schedule
//! arriving is answered; and a notification is removed through the same identity it was posted
//! under.

use std::collections::HashMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::{FutureExt, StreamExt};
use waft::ui::{self, NotificationActivation, Platform, PlatformRef};
use waft::{App, AsyncApp};
use waos::bindings::wasdk::platform::notifications::NotificationError;
use waos::{Notification, RunningProgram, ShellNotifications};
use waos_store::Store;

use crate::logic::notification_calls::{
    NotificationCall, NotificationRequest, NotificationsOnRequest, Offered,
};
use crate::logic::program_identities::installed_app_id;
use crate::logic::program_name::program_name;
use crate::logic::system_notifications::{Titled, system_id, system_notification};

/// Waft's notifications, served to the engine as its shell's.
pub(crate) struct Notifications {
    calls: NotificationsOnRequest,
    arriving: UnboundedReceiver<NotificationCall>,
}

impl Notifications {
    /// Waft's notifications on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<Notifications> {
        let notifications = platform.notifications()?;
        let offered = Offered {
            replies: notifications.replies().is_some(),
            scheduling: notifications.scheduling().is_some(),
        };
        let (calls, arriving) = NotificationsOnRequest::new(offered);
        Some(Notifications { calls, arriving })
    }

    /// The notifications as the engine's shell has them: each call goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellNotifications> {
        Arc::new(self.calls.clone())
    }

    /// From now on, each call of a program's is carried out through Waft's notifications, the
    /// program named as `store` names it to its user. Answers where the clicks on the
    /// notifications posted as a program arrive, which WaOS's own handler never hears.
    pub(crate) fn serve(
        self,
        app: &mut App,
        store: Store,
    ) -> UnboundedReceiver<NotificationActivation> {
        let (clicks, arriving_clicks) = mpsc::unbounded();
        let mut serving = Serving {
            platform: app.platform(),
            store,
            permission_asked: false,
            as_programs: HashMap::new(),
            clicks,
        };
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            while let Some(call) = arriving.next().await {
                serving.carry_out(call, cx).await;
            }
        });
        arriving_clicks
    }
}

/// What carries out the programs' calls, in the order they come.
struct Serving {
    platform: PlatformRef,
    /// Where an installed program's name is read.
    store: Store,
    /// Whether the system was asked for WaOS's permission.
    permission_asked: bool,
    /// The notifications posted as each program, by its identity, made at its first; none for a
    /// program the system lets WaOS post as no other.
    as_programs: HashMap<String, Option<Rc<dyn ui::Notifications>>>,
    /// Where a click on a notification posted as a program goes.
    clicks: UnboundedSender<NotificationActivation>,
}

/// What showing a notification answers the program.
type Shown = Pin<Box<dyn Future<Output = Result<(), NotificationError>>>>;

impl Serving {
    /// Carries out `call`; a post is answered once the system has it, while later calls go on.
    async fn carry_out(&mut self, call: NotificationCall, cx: &mut AsyncApp) {
        let NotificationCall { program, request } = call;
        match request {
            NotificationRequest::Post {
                notification,
                at,
                posted,
            } => {
                self.ask_permission_once().await;
                let shown = self.show(&program, &notification, at);
                cx.update(|app| {
                    app.spawn(async move |_cx| {
                        // A program that ended meanwhile hears nothing.
                        let _ = posted.send(shown.await);
                    })
                });
            }
            NotificationRequest::Remove { id } => {
                let own = self.posted_as(&program);
                let notifications = own.as_deref().or_else(|| self.platform.notifications());
                if let Some(notifications) = notifications {
                    notifications.remove(&system_id(program.name(), &id));
                }
            }
        }
    }

    /// Asks the system for WaOS's permission to show notifications, the first time only, where
    /// the system keeps one.
    async fn ask_permission_once(&mut self) {
        if self.permission_asked {
            return;
        }
        self.permission_asked = true;
        let asked = self
            .platform
            .notifications()
            .and_then(|notifications| notifications.permission())
            .map(|permission| permission.request());
        if let Some(asked) = asked {
            let state = asked.await;
            tracing::info!(?state, "the system's permission for WaOS's notifications");
        }
    }

    /// Shows `program`'s `notification` through the system's notifications, now or at `at`.
    fn show(
        &mut self,
        program: &RunningProgram,
        notification: &Notification,
        at: Option<SystemTime>,
    ) -> Shown {
        let own = self.posted_as(program);
        let (notifications, titled) = match own.as_deref() {
            Some(own) => (own, Titled::Own),
            None => match self.platform.notifications() {
                Some(waos) => (waos, Titled::AfterTheProgram),
                None => return failed("the system has no notifications"),
            },
        };
        let name = program_name(&self.store, program);
        let shown = system_notification(
            program.name(),
            &name,
            titled,
            notification,
            notifications.replies(),
        );
        let posting = match (at, notifications.scheduling()) {
            (None, _) => notifications.post(&shown),
            (Some(at), Some(scheduling)) => scheduling.schedule(&shown, at),
            (Some(_), None) => return failed("the system schedules no notification"),
        };
        Box::pin(posting.map(|posted| posted.map_err(program_error)))
    }

    /// The notifications `program`'s are posted as it through, where it is installed and the
    /// system lets WaOS post as it; none where they are WaOS's, titled after the program.
    fn posted_as(&mut self, program: &RunningProgram) -> Option<Rc<dyn ui::Notifications>> {
        let (_, app_id) = installed_app_id(&self.store, program.name())?;
        self.notifications_as(&app_id)
    }

    /// The notifications posted as the app `app_id`, made the first time and kept, their clicks
    /// sent where WaOS hears them; none where the system will not let WaOS post as it.
    fn notifications_as(&mut self, app_id: &str) -> Option<Rc<dyn ui::Notifications>> {
        let (platform, clicks) = (&self.platform, &self.clicks);
        self.as_programs
            .entry(app_id.to_owned())
            .or_insert_with(|| {
                let clicks = clicks.clone();
                let made = notifications_posted_as(
                    platform.as_ref(),
                    app_id,
                    Rc::new(move |activation| {
                        let _ = clicks.unbounded_send(activation);
                    }),
                );
                if made.is_none() {
                    tracing::info!(
                        app_id,
                        "the system lets WaOS post as no program, so it posts as itself"
                    );
                }
                made
            })
            .clone()
    }
}

/// The notifications of the app `app_id`, which WaOS's process posts as, through what only Windows
/// offers, their clicks going to `clicked`; none where the platform is no Waft desktop host's, or
/// the system knows no app by the id.
#[cfg(windows)]
fn notifications_posted_as(
    platform: &dyn Platform,
    app_id: &str,
    clicked: Rc<dyn Fn(NotificationActivation)>,
) -> Option<Rc<dyn ui::Notifications>> {
    let winit = platform.downcast_ref::<waft_embedder_winit::WinitPlatform>()?;
    winit.windows()?.notifications_of(app_id, clicked)
}

/// None: only Windows lets a process post as another app.
#[cfg(not(windows))]
fn notifications_posted_as(
    _platform: &dyn Platform,
    _app_id: &str,
    _clicked: Rc<dyn Fn(NotificationActivation)>,
) -> Option<Rc<dyn ui::Notifications>> {
    None
}

fn failed(why: &str) -> Shown {
    Box::pin(ready(Err(NotificationError::Failed(why.to_owned()))))
}

/// Why the system did not show a notification, as the program hears it: the system refusing WaOS
/// is a failure, since `denied` is the program's own permission's.
fn program_error(error: ui::NotificationError) -> NotificationError {
    match error {
        ui::NotificationError::Denied => {
            NotificationError::Failed("the system does not let WaOS show notifications".to_owned())
        }
        ui::NotificationError::Failed(reason) => NotificationError::Failed(reason),
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};

    use waft::ui::{EmbedderClient, NotificationActivation, PermissionState};
    use waft_test::{MockNotifications, TestPlatform};
    use waos::{Answer, Question};
    use waos_package::ProgramId;

    use super::*;
    use crate::state::test_system::{Lines, TestSystem};

    const NOTIFICATIONS: &str = "dev.example.notifications";

    /// The app over a platform whose notifications are `notifications`, with `examples/notifications` installed, named Notifications, its user having
    /// allowed it notifications.
    fn with_notifications_installed(notifications: &MockNotifications) -> TestSystem {
        let platform = TestPlatform::new().with_notifications(Rc::new(notifications.clone()));
        let system = TestSystem::new(platform);
        system.install(
            waos_test_programs::NOTIFICATIONS,
            NOTIFICATIONS,
            "Notifications",
        );
        let id: ProgramId = NOTIFICATIONS.parse().unwrap();
        let grants = system.store.grants(&id);
        grants.set_answer(Question::Notifications, Answer::Granted);
        system
    }

    /// Runs `examples/notifications`, installed, over a platform whose notifications are
    /// `notifications`, until it listens for activations, done posting; answers it and what it
    /// printed.
    fn run_notifications(
        notifications: &MockNotifications,
    ) -> (TestSystem, RunningProgram, Arc<Lines>) {
        let system = with_notifications_installed(notifications);
        let lines = Arc::new(Lines::default());
        let program = system.run_as(waos_test_programs::NOTIFICATIONS, NOTIFICATIONS, &lines);
        system.run_tasks_until("the program done posting", || {
            lines.has("listening for activations")
        });
        (system, program, lines)
    }

    fn click(id: &str, action: Option<&str>, reply: Option<&str>) -> NotificationActivation {
        NotificationActivation {
            id: id.to_owned(),
            action: action.map(str::to_owned),
            reply: reply.map(str::to_owned),
        }
    }

    #[test]
    fn a_programs_notification_is_shown_under_its_id_titled_after_its_name_once_the_system_lets_waos()
     {
        let notifications =
            MockNotifications::new().asking(PermissionState::NotAsked, PermissionState::Granted);
        let (system, program, lines) = run_notifications(&notifications);
        for line in [
            "post greeting: ok",
            "post greeting again: ok",
            "post greeting with a reply: ok",
            "schedule reminder: ok",
        ] {
            assert!(lines.has(line), "{line}");
        }
        assert_eq!(
            notifications.times_asked(),
            1,
            "the system asked once, for WaOS"
        );
        let shown = notifications.shown();
        assert_eq!(shown.len(), 1, "posted again in its place: {shown:?}");
        assert_eq!(shown[0].id, "dev.example.notifications/greeting");
        assert_eq!(shown[0].title, "Notifications: Hello");
        let replies: Vec<_> = shown[0]
            .actions
            .iter()
            .map(|action| (action.id.as_str(), action.reply_placeholder()))
            .collect();
        assert_eq!(
            replies,
            [("later", None), ("answer", Some("Say something"))]
        );
        let scheduled = notifications.scheduled();
        assert_eq!(scheduled[0].0.id, "dev.example.notifications/reminder");
        assert_eq!(
            scheduled[0].1,
            UNIX_EPOCH + Duration::from_secs(4_102_444_800)
        );

        let shell = system
            .notifications
            .as_ref()
            .expect("the app's notifications");
        shell.remove(&program, "greeting");
        system.run_tasks_until("the greeting taken away", || {
            notifications.shown().is_empty()
        });
    }

    #[test]
    fn a_notification_the_system_does_not_let_waos_show_fails() {
        let notifications =
            MockNotifications::new().asking(PermissionState::NotAsked, PermissionState::Denied);
        let (_system, _program, lines) = run_notifications(&notifications);
        assert!(
            lines.has("post greeting: failed (the system does not let WaOS show notifications)")
        );
        assert_eq!(notifications.times_asked(), 1);
        assert!(notifications.shown().is_empty());
    }

    #[test]
    fn replies_and_scheduling_are_offered_where_the_system_has_them() {
        let notifications = MockNotifications::new()
            .without_replies()
            .without_scheduling();
        let (_system, _program, lines) = run_notifications(&notifications);
        assert!(lines.has("replies: none"));
        assert!(lines.has("scheduling: none"));
        assert_eq!(
            notifications.shown()[0].id,
            "dev.example.notifications/greeting"
        );
    }

    #[test]
    fn a_click_on_a_programs_notification_reaches_the_program_running() {
        let notifications = MockNotifications::new();
        let (mut system, _program, lines) = run_notifications(&notifications);
        let reply = click(
            "dev.example.notifications/greeting",
            Some("answer"),
            Some("hi"),
        );
        system.shell.notification_activated(reply);
        system.run_tasks_until("the click heard", || {
            lines.has(r#"activated: greeting Some("answer") Some("hi")"#)
        });
    }

    #[test]
    fn a_click_on_an_installed_programs_notification_starts_it() {
        let notifications = MockNotifications::new();
        let mut system = with_notifications_installed(&notifications);
        let later = click("dev.example.notifications/greeting", Some("later"), None);
        system.shell.notification_activated(later);
        system.run_tasks_until("the program started, posting its greeting", || {
            !notifications.shown().is_empty()
        });
        let running = futures::executor::block_on(system.runs.names());
        assert_eq!(running, [NOTIFICATIONS]);
        assert_eq!(system.problem(), None);
    }

    #[test]
    fn a_click_on_a_notification_of_a_program_no_longer_installed_tells_the_user() {
        let platform = TestPlatform::new().with_notifications(Rc::new(MockNotifications::new()));
        let mut system = TestSystem::new(platform);
        system
            .shell
            .notification_activated(click("dev.example.gone/greeting", None, None));
        system.run_tasks_until("the click's answer", || system.problem().is_some());
        assert_eq!(
            system.problem().as_deref(),
            Some(
                "dev.example.gone is no longer installed, so its notification has nothing to open"
            )
        );
        assert_eq!(system.asked_to_be_seen(), 1, "to say so");
    }
}
