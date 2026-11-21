use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{
    Answer, EngineTime, NoPrompts, OpenRequest, ProgramEnvironment, ProgramGrants, ProgramOutput,
    Question,
};
use waos_package::ProgramId;
use waos_store::{SourceProxy, Store};
use waos_test_programs::CODE_CACHE;
use waos_window_messages::{FromWindow, LaunchReason, PermissionRequestId, PostId, ToWindow};

use super::*;
use crate::logic::program_apps::app_reach::LaunchAttempt;
use crate::logic::program_apps::app_reach::tests::Launches;
use crate::logic::program_apps::server::deliver_hello;
use crate::logic::program_apps::window_process::tests::{
    Printed, Served, WITHIN, next, next_wanted, window_process,
};
use crate::logic::program_apps::window_process::{SystemPermission, WindowProcess};
use crate::logic::test_engine::{self, NoWindows};

/// The id notifications runs under, as an installed program's.
const NOTIFICATIONS: &str = "dev.example.notifications";

fn notifications_id() -> ProgramId {
    NOTIFICATIONS.parse().unwrap()
}

/// Notifications, under its id, with no window, its notifications posted in its app as `reach`
/// reaches it, as the engine posts a program's on macOS, its user having allowed it notifications
/// in `store`, what it prints going into `printed`.
fn notifications_through(
    reach: &Arc<AppReach>,
    store: &Store,
    printed: &Arc<Printed>,
) -> RunningProgram {
    let mut shell = test_engine::shell(Arc::new(NoWindows), Arc::new(NoPrompts));
    let in_apps = AppNotifications::new(Arc::clone(reach));
    shell.notifications = Some(Arc::new(in_apps) as Arc<dyn ShellNotifications>);
    let engine = test_engine::engine_over(shell, EngineTime::Real, Path::new(CODE_CACHE));
    let notifications = engine
        .load(Path::new(waos_test_programs::NOTIFICATIONS))
        .expect("notifications loads")
        .named(NOTIFICATIONS);
    store
        .folders(&notifications_id())
        .expect("its folders, as at its first run");
    let grants = store.grants(&notifications_id());
    grants.set_answer(Question::Notifications, Answer::Granted);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(printed) as Arc<dyn ProgramOutput>),
        grants: Some(grants as Arc<dyn ProgramGrants>),
        ..ProgramEnvironment::default()
    };

    engine
        .run(&notifications, environment)
        .expect("notifications runs")
}

/// What the window process said macOS said of the program's notifications next.
async fn next_said(said: &mut UnboundedReceiver<SystemPermission>) -> SystemPermission {
    tokio::time::timeout(WITHIN, said.next())
        .await
        .expect("macOS says something in time")
        .expect("what it said")
}

/// A store in a folder of the test's own, kept as long as the folder.
fn store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().unwrap();
    let store = Store::open(&folder.path().join("store"), SourceProxy::None).expect("a store");
    (folder, store)
}

/// The notification the app is asked to post next: the id its answer names, the notification, and
/// when it shows.
async fn post_asked(
    sent: &mut UnboundedReceiver<ToWindow>,
) -> (PostId, Notification, Option<SystemTime>) {
    let asked = next_wanted(sent, |message| {
        matches!(message, ToWindow::PostNotification { .. })
    })
    .await;
    let ToWindow::PostNotification {
        post,
        notification,
        at,
    } = asked
    else {
        unreachable!("a notification");
    };
    (post, notification, at)
}

/// Answers each post the app is asked for with `posted`, as the system would, until `count` were.
async fn answer_posts(
    process: &WindowProcess,
    sent: &mut UnboundedReceiver<ToWindow>,
    count: usize,
    posted: Result<(), NotificationError>,
) {
    for _ in 0..count {
        let (post, _, _) = post_asked(sent).await;
        let posted = posted.clone();
        process.received(FromWindow::NotificationPosted { post, posted }, Vec::new());
    }
}

/// macOS's question the app is asked to put next, by the id its answer names.
async fn permission_asked(sent: &mut UnboundedReceiver<ToWindow>) -> PermissionRequestId {
    let asked = next_wanted(sent, |message| {
        matches!(message, ToWindow::RequestNotificationPermission { .. })
    })
    .await;
    let ToWindow::RequestNotificationPermission { request } = asked else {
        unreachable!("macOS's question");
    };
    request
}

/// Answers macOS's question the app is asked to put next, as macOS would: that it `refused` the
/// program its notifications, or not.
async fn answer_permission(
    process: &WindowProcess,
    sent: &mut UnboundedReceiver<ToWindow>,
    refused: bool,
) {
    let request = permission_asked(sent).await;
    let answer = FromWindow::NotificationPermissionAnswered { request, refused };
    process.received(answer, Vec::new());
}

/// What macOS says of the notifications program's, as the window process tells it.
fn said_of_notifications(refuses: bool) -> SystemPermission {
    SystemPermission {
        program: notifications_id(),
        refuses,
    }
}

fn click(action: &str, reply: Option<&str>) -> Activation {
    Activation {
        id: "greeting".to_owned(),
        action: Some(action.to_owned()),
        reply: reply.map(str::to_owned),
    }
}

#[tokio::test]
async fn a_notification_of_a_program_with_no_app_launches_the_app_and_is_posted_there() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = notifications_through(&reach, &store, &printed);
    launches.until(1).await;
    assert_eq!(
        launches.asked(),
        [(notifications_id(), LaunchAttempt::First)],
        "the program has no window: its first notification launches its app"
    );

    let Served {
        process, mut sent, ..
    } = window_process(program.clone());
    reach.connected(&notifications_id(), &process);
    let (post, greeting, at) = post_asked(&mut sent).await;
    assert_eq!(
        (greeting.id.as_str(), greeting.title.as_str(), at),
        ("greeting", "Hello", None),
        "the program's own, posted now"
    );
    process.received(
        FromWindow::NotificationPosted {
            post,
            posted: Ok(()),
        },
        Vec::new(),
    );
    printed.line("post greeting: ok").await;
    answer_permission(&process, &mut sent, false).await;
    printed.line("permission asked: granted").await;
    answer_posts(&process, &mut sent, 2, Ok(())).await;
    let (post, reminder, at) = post_asked(&mut sent).await;
    assert_eq!(reminder.id, "reminder");
    assert_eq!(at, Some(UNIX_EPOCH + Duration::from_secs(4_102_444_800)));
    process.received(
        FromWindow::NotificationPosted {
            post,
            posted: Ok(()),
        },
        Vec::new(),
    );
    let removed = next_wanted(&mut sent, |message| {
        matches!(message, ToWindow::RemoveNotification { .. })
    })
    .await;
    assert!(matches!(removed, ToWindow::RemoveNotification { id } if id == "old"));
    for line in [
        "post greeting again: ok",
        "post greeting with a reply: ok",
        "schedule reminder: ok",
        "listening for activations",
    ] {
        printed.line(line).await;
    }

    process.received(
        FromWindow::NotificationActivated(click("answer", Some("hi"))),
        Vec::new(),
    );
    printed
        .line(r#"activated: greeting Some("answer") Some("hi")"#)
        .await;
    assert_eq!(
        launches.asked().len(),
        1,
        "every notification after went to that app"
    );
    program.stop();
}

#[tokio::test]
async fn the_click_that_launched_the_app_reaches_the_program_once_it_listens() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = notifications_through(&reach, &store, &printed);
    let Served {
        process, mut sent, ..
    } = window_process(program.clone());
    let reason = LaunchReason::NotificationActivated(click("later", None));
    deliver_hello(&program, reason, OpenRequest::default());
    reach.connected(&notifications_id(), &process);

    answer_posts(&process, &mut sent, 1, Ok(())).await;
    answer_permission(&process, &mut sent, false).await;
    answer_posts(&process, &mut sent, 3, Ok(())).await;
    printed.line("listening for activations").await;
    printed
        .line(r#"activated: greeting Some("later") None"#)
        .await;
    program.stop();
}

#[tokio::test]
async fn what_macos_says_reads_as_the_programs_permission_and_its_page_hears_it() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = notifications_through(&reach, &store, &printed);
    let Served {
        process,
        mut sent,
        system_permissions: mut said,
        ..
    } = window_process(program.clone());
    reach.connected(&notifications_id(), &process);
    let welcomed = FromWindow::NotificationPermission { refused: true };
    process.received(welcomed, Vec::new());
    assert_eq!(
        next_said(&mut said).await,
        said_of_notifications(true),
        "its page hears what the app read as WaOS welcomed it"
    );

    answer_posts(&process, &mut sent, 1, Err(NotificationError::Denied)).await;
    printed.line("post greeting: denied").await;
    assert_eq!(next_said(&mut said).await, said_of_notifications(true));
    answer_permission(&process, &mut sent, true).await;
    printed.line("permission asked: denied").await;
    assert_eq!(
        next_said(&mut said).await,
        said_of_notifications(true),
        "macOS's answer to its question"
    );
    assert_eq!(
        store
            .grants(&notifications_id())
            .answer(Question::Notifications),
        Some(Answer::Granted),
        "WaOS's own answer is kept"
    );

    answer_posts(&process, &mut sent, 1, Ok(())).await;
    printed.line("post greeting again: ok").await;
    assert_eq!(
        next_said(&mut said).await,
        said_of_notifications(false),
        "a post macOS takes again, as once the user turned them on in System Settings"
    );
    post_asked(&mut sent).await;
    process.gone();
    printed
        .line("post greeting with a reply: failed (the program's app is gone)")
        .await;
    program.stop();
}

#[tokio::test]
async fn a_question_of_macos_whose_app_goes_first_leaves_the_permission_as_it_was() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = notifications_through(&reach, &store, &printed);
    let Served {
        process,
        mut sent,
        system_permissions: mut said,
        ..
    } = window_process(program.clone());
    reach.connected(&notifications_id(), &process);

    answer_posts(&process, &mut sent, 1, Ok(())).await;
    assert_eq!(next_said(&mut said).await, said_of_notifications(false));
    permission_asked(&mut sent).await;
    process.gone();
    printed.line("permission asked: granted").await;
    printed
        .line("post greeting again: failed (the program's app is gone)")
        .await;
    program.stop();
}

#[tokio::test]
async fn macos_permission_is_read_again_only_in_an_app_that_runs() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = notifications_through(&reach, &store, &printed);
    launches.until(1).await;
    let in_apps = AppNotifications::new(Arc::clone(&reach));
    in_apps.read_system_permissions(std::slice::from_ref(&program));
    assert_eq!(
        launches.asked().len(),
        1,
        "an app that is launching is not launched again for it"
    );

    let Served {
        process, mut sent, ..
    } = window_process(program.clone());
    reach.connected(&notifications_id(), &process);
    let first = next(&mut sent).await;
    assert!(
        matches!(first, ToWindow::PostNotification { .. }),
        "the read asked while the app launched waited for nothing: {first:?}"
    );
    in_apps.read_system_permissions(std::slice::from_ref(&program));
    let read = next(&mut sent).await;
    assert!(
        matches!(read, ToWindow::ReadNotificationPermission),
        "read again in the app that runs: {read:?}"
    );
    program.stop();
}

#[tokio::test]
async fn a_notification_whose_app_never_comes_fails_with_why() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = notifications_through(&reach, &store, &printed);
    launches.until(1).await;
    reach.launch_failed(&notifications_id(), "Launch Services: no app");
    reach.launch_failed(&notifications_id(), "Launch Services: no app");
    printed
        .line("post greeting: failed (the program's app did not start: Launch Services: no app)")
        .await;
    program.stop();
}
