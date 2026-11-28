use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{
    Answer, EngineTime, OpenRequest, ProgramEnvironment, ProgramGrants, ProgramOutput, Question,
    ShellSystemPermissions, SystemPermissionState,
};
use waos_package::ProgramId;
use waos_window_messages::{FromWindow, LaunchReason, PermissionRequestId, PostId, ToWindow};

use super::*;
use waos_services::permissions::Setting;

use crate::logic::macos_shell::server::deliver_hello;
use crate::logic::macos_shell::window_process::WindowProcess;
use crate::logic::macos_shell::window_process::tests::{
    Printed, Served, next, next_wanted, window_process_keeping,
};
use crate::logic::macos_shell::window_processes::LaunchAttempt;
use crate::logic::macos_shell::window_processes::tests::{Launches, said_hello};
use crate::logic::notification_permissions::NotificationPermissionReading;
use crate::logic::startup::Domains;
use crate::logic::test_engine::{self, NoWindows, WITHIN};
use crate::logic::test_installs;

/// The id notifications runs under, as an installed program's.
const NOTIFICATIONS: &str = "dev.example.notifications";

fn notifications_id() -> ProgramId {
    NOTIFICATIONS.parse().unwrap()
}

/// Notifications, under its id, with no window, its notifications posted in its app as
/// `window_processes` reach it, and macOS's permission for them read from `permissions`, as the
/// engine has a program's on macOS, its user having allowed it notifications in `store`, what it
/// prints going into `printed`.
fn notifications_through(
    window_processes: &Arc<WindowProcesses>,
    permissions: &NotificationPermissions,
    store: &Domains,
    printed: &Arc<Printed>,
) -> RunningProgram {
    let mut shell = test_engine::shell(Arc::new(NoWindows));
    let in_apps = Arc::new(AppNotifications::new(
        Arc::clone(window_processes),
        permissions.clone(),
    ));
    shell.notifications = Some(Arc::clone(&in_apps) as Arc<dyn ShellNotifications>);
    shell.system_permissions = Some(in_apps as Arc<dyn ShellSystemPermissions>);
    let engine = test_engine::engine_over(shell, EngineTime::Real);
    let notifications = engine
        .load(
            Path::new(waos_test_programs::NOTIFICATIONS),
            &waos_test_programs::code_of(waos_test_programs::NOTIFICATIONS),
        )
        .expect("notifications loads")
        .named(NOTIFICATIONS);
    store
        .storage
        .folders(&notifications_id())
        .expect("its folders, as at its first run");
    let granted = store
        .permissions
        .change_app_settings(&notifications_id(), |own| {
            own.answers
                .set(Question::Notifications, Setting::Own(Answer::Granted));
        });
    granted.expect("the answer kept");
    let grants = store.permissions.of(&notifications_id());
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
async fn next_said(
    said: &mut UnboundedReceiver<NotificationPermissionReading>,
) -> NotificationPermissionReading {
    tokio::time::timeout(WITHIN, said.next())
        .await
        .expect("macOS says something in time")
        .expect("what it said")
}

/// The store and the permissions in a folder of the test's own, kept as long as the folder.
fn store() -> (tempfile::TempDir, Domains) {
    let folder = tempfile::tempdir().unwrap();
    let domains = test_installs::domains_never_run(&folder.path().join("store"));
    (folder, domains)
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

/// Answers macOS's question the app is asked to put next with `state`, as macOS would.
async fn answer_permission(
    process: &WindowProcess,
    sent: &mut UnboundedReceiver<ToWindow>,
    state: SystemPermissionState,
) {
    let request = permission_asked(sent).await;
    let answer = FromWindow::NotificationPermissionAnswered { request, state };
    process.received(answer, Vec::new());
}

/// What macOS says of the notifications program's, as the window process tells it.
fn said_of_notifications(refuses: bool) -> NotificationPermissionReading {
    NotificationPermissionReading {
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
    let window_processes = launches.window_processes();
    let printed = Arc::new(Printed::default());
    let (permissions, mut said) = NotificationPermissions::new();
    let program = notifications_through(&window_processes, &permissions, &store, &printed);
    launches.until(1).await;
    assert_eq!(
        launches.asked(),
        [(notifications_id(), LaunchAttempt::First)],
        "the program has no window: its first notification launches its app"
    );

    let Served {
        process, mut sent, ..
    } = window_process_keeping(program.clone(), permissions.clone());
    said_hello(&window_processes, &notifications_id(), &process);
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
    printed.line("permission asked: granted").await;
    assert_eq!(
        next_said(&mut said).await,
        said_of_notifications(false),
        "macOS took the post, so its question is not put again"
    );
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
    let window_processes = launches.window_processes();
    let printed = Arc::new(Printed::default());
    let (permissions, _readings) = NotificationPermissions::new();
    let program = notifications_through(&window_processes, &permissions, &store, &printed);
    let Served {
        process, mut sent, ..
    } = window_process_keeping(program.clone(), permissions.clone());
    let reason = LaunchReason::NotificationActivated(click("later", None));
    deliver_hello(&program, reason, OpenRequest::default());
    said_hello(&window_processes, &notifications_id(), &process);

    answer_posts(&process, &mut sent, 4, Ok(())).await;
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
    let window_processes = launches.window_processes();
    let printed = Arc::new(Printed::default());
    let (permissions, mut said) = NotificationPermissions::new();
    let program = notifications_through(&window_processes, &permissions, &store, &printed);
    let Served {
        process, mut sent, ..
    } = window_process_keeping(program.clone(), permissions.clone());
    said_hello(&window_processes, &notifications_id(), &process);
    let welcomed = FromWindow::NotificationPermission {
        state: SystemPermissionState::Denied,
    };
    process.received(welcomed, Vec::new());
    assert_eq!(
        next_said(&mut said).await,
        said_of_notifications(true),
        "its page hears what the app read as WaOS welcomed it"
    );

    answer_posts(&process, &mut sent, 1, Err(NotificationError::Denied)).await;
    printed.line("post greeting: denied").await;
    assert_eq!(next_said(&mut said).await, said_of_notifications(true));
    answer_permission(&process, &mut sent, SystemPermissionState::Denied).await;
    printed.line("permission asked: denied").await;
    assert_eq!(
        next_said(&mut said).await,
        said_of_notifications(true),
        "macOS's answer to its question"
    );
    assert_eq!(
        store
            .permissions
            .of(&notifications_id())
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
    let window_processes = launches.window_processes();
    let printed = Arc::new(Printed::default());
    let (permissions, mut said) = NotificationPermissions::new();
    let program = notifications_through(&window_processes, &permissions, &store, &printed);
    let Served {
        process, mut sent, ..
    } = window_process_keeping(program.clone(), permissions.clone());
    said_hello(&window_processes, &notifications_id(), &process);

    answer_posts(&process, &mut sent, 1, Err(NotificationError::Denied)).await;
    assert_eq!(next_said(&mut said).await, said_of_notifications(true));
    permission_asked(&mut sent).await;
    process.gone();
    printed.line("permission asked: denied").await;
    printed
        .line("post greeting again: failed (the program's app is gone)")
        .await;
    program.stop();
}

#[tokio::test]
async fn macos_permission_is_read_again_only_in_an_app_that_runs() {
    let (_folder, store) = store();
    let launches = Launches::default();
    let window_processes = launches.window_processes();
    let printed = Arc::new(Printed::default());
    let (permissions, _readings) = NotificationPermissions::new();
    let program = notifications_through(&window_processes, &permissions, &store, &printed);
    launches.until(1).await;
    let in_apps = AppNotifications::new(Arc::clone(&window_processes), permissions.clone());
    in_apps.read_system_permissions(std::slice::from_ref(&program));
    assert_eq!(
        launches.asked().len(),
        1,
        "an app that is launching is not launched again for it"
    );

    let Served {
        process, mut sent, ..
    } = window_process_keeping(program.clone(), permissions.clone());
    said_hello(&window_processes, &notifications_id(), &process);
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
    let window_processes = launches.window_processes();
    let printed = Arc::new(Printed::default());
    let (permissions, _readings) = NotificationPermissions::new();
    let program = notifications_through(&window_processes, &permissions, &store, &printed);
    launches.until(1).await;
    launches.fail(0, "Launch Services: no app").await;
    launches.fail(1, "Launch Services: no app").await;
    printed
        .line("post greeting: failed (the program's app did not start: Launch Services: no app)")
        .await;
    program.stop();
}
