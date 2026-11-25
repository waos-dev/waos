use std::time::{Duration, UNIX_EPOCH};

use waft::Shell;
use waft::ui::{AppLifecycleState, EmbedderClient, NotificationActivation, PermissionState};
use waft_test::{MockNotifications, TestPlatform};
use waos::NotificationAction;

use super::*;
use crate::connection::tests::{connection, sent};

/// The program's greeting, with an action answered by typing.
fn greeting() -> Notification {
    Notification {
        id: "greeting".to_owned(),
        title: "Hello".to_owned(),
        body: Some("From a program".to_owned()),
        image: None,
        actions: vec![NotificationAction {
            id: "answer".to_owned(),
            label: "Answer".to_owned(),
            reply_placeholder: Some("Say something".to_owned()),
        }],
        silent: false,
    }
}

fn asked(post: PostId, notification: Notification, at: Option<SystemTime>) -> AskedNotification {
    AskedNotification {
        post,
        notification,
        at,
    }
}

/// What the window process answered WaOS for a post: its id, and what the system said.
fn answered(waos: &waos_window_messages::os::macos::ReceiveRight) -> (PostId, String) {
    let answer = sent(waos);
    let FromWindow::NotificationPosted { post, posted } = answer else {
        panic!("a post's answer: {answer:?}");
    };
    (post, format!("{posted:?}"))
}

/// What the window process told WaOS next of macOS's permission.
fn permission_told(waos: &waos_window_messages::os::macos::ReceiveRight) -> SystemPermissionState {
    let told = sent(waos);
    let FromWindow::NotificationPermission { state } = told else {
        panic!("macOS's permission: {told:?}");
    };
    state
}

const NOT_ASKED: SystemPermissionState = SystemPermissionState::NotAsked;
const ALLOWED: SystemPermissionState = SystemPermissionState::Allowed;
const DENIED: SystemPermissionState = SystemPermissionState::Denied;

#[test]
fn a_notification_is_posted_under_the_programs_own_id_and_title_once_macos_asked_the_user() {
    let notifications =
        MockNotifications::new().asking(PermissionState::NotAsked, PermissionState::Granted);
    let platform = TestPlatform::new().with_notifications(Rc::new(notifications.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let reminder_at = UNIX_EPOCH + Duration::from_secs(4_102_444_800);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let served = ProgramNotifications::serve(app, connection);
        served.post(asked(1, greeting(), None));
        served.post(asked(2, greeting(), None));
        let reminder = Notification {
            id: "reminder".to_owned(),
            ..greeting()
        };
        served.post(asked(3, reminder, Some(reminder_at)));
        served.remove("old".to_owned());
    });
    shell.wake(Duration::ZERO);
    assert_eq!(permission_told(&waos), NOT_ASKED, "as WaOS welcomed it");
    assert_eq!(permission_told(&waos), ALLOWED, "at its question");
    for post in 1..=3 {
        assert_eq!(answered(&waos), (post, "Ok(())".to_owned()));
    }
    assert_eq!(
        notifications.times_asked(),
        1,
        "macOS asked once, for the app"
    );
    let shown = notifications.shown();
    assert_eq!(shown.len(), 1, "posted again in its place: {shown:?}");
    assert_eq!(
        (shown[0].id.as_str(), shown[0].title.as_str()),
        ("greeting", "Hello"),
        "the program's own, under its own bundle"
    );
    assert_eq!(
        shown[0].actions[0].reply_placeholder(),
        Some("Say something")
    );
    let scheduled = notifications.scheduled();
    assert_eq!(scheduled[0].0.id, "reminder");
    assert_eq!(scheduled[0].1, reminder_at);
}

#[test]
fn a_notification_macos_refuses_is_denied_and_its_question_is_put_once() {
    let notifications =
        MockNotifications::new().asking(PermissionState::NotAsked, PermissionState::Denied);
    let platform = TestPlatform::new().with_notifications(Rc::new(notifications.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let served = ProgramNotifications::serve(app, connection);
        served.post(asked(1, greeting(), None));
        served.post(asked(2, greeting(), None));
    });
    shell.wake(Duration::ZERO);
    assert_eq!(permission_told(&waos), NOT_ASKED, "as WaOS welcomed it");
    assert_eq!(permission_told(&waos), DENIED, "at its question");
    assert_eq!(
        answered(&waos),
        (1, "Err(NotificationError::Denied)".to_owned())
    );
    assert_eq!(
        answered(&waos),
        (2, "Err(NotificationError::Denied)".to_owned())
    );
    assert_eq!(notifications.times_asked(), 1);
    assert!(notifications.shown().is_empty());
}

#[test]
fn a_notification_where_the_host_has_none_fails_with_why() {
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let mut shell = Shell::new(Rc::new(TestPlatform::new()), |app| {
        ProgramNotifications::serve(app, connection).post(asked(4, greeting(), None));
    });
    shell.wake(Duration::ZERO);
    assert_eq!(
        permission_told(&waos),
        ALLOWED,
        "nothing denies where nothing posts"
    );
    assert_eq!(permission_told(&waos), ALLOWED);
    let (post, posted) = answered(&waos);
    assert_eq!(post, 4);
    assert!(posted.contains("Failed"), "{posted}");
}

#[test]
fn macos_is_asked_when_waos_asks_answered_by_its_id_and_read_again_as_the_app_comes_to_the_front() {
    let notifications =
        MockNotifications::new().asking(PermissionState::NotAsked, PermissionState::Denied);
    let platform = TestPlatform::new().with_notifications(Rc::new(notifications.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let served = ProgramNotifications::serve(app, connection);
        served.request_permission(7);
        served.post(asked(8, greeting(), None));
    });
    shell.wake(Duration::ZERO);
    assert_eq!(permission_told(&waos), NOT_ASKED, "as WaOS welcomed it");
    let answer = sent(&waos);
    assert!(
        matches!(
            answer,
            FromWindow::NotificationPermissionAnswered {
                request: 7,
                state: DENIED
            }
        ),
        "{answer:?}"
    );
    assert_eq!(
        answered(&waos),
        (8, "Err(NotificationError::Denied)".to_owned()),
        "the post is not asked again"
    );
    assert_eq!(notifications.times_asked(), 1);

    shell.app_lifecycle_state_changed(AppLifecycleState::Resumed);
    shell.wake(Duration::ZERO);
    assert_eq!(
        permission_told(&waos),
        DENIED,
        "read again as the app comes to the front"
    );
}

#[test]
fn macos_permission_is_read_again_without_asking_each_time_waos_asks() {
    let notifications =
        MockNotifications::new().asking(PermissionState::Denied, PermissionState::Denied);
    let platform = TestPlatform::new().with_notifications(Rc::new(notifications.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let served = Rc::new(RefCell::new(None));
    let mut shell = Shell::new(Rc::new(platform), |app| {
        *served.borrow_mut() = Some(ProgramNotifications::serve(app, connection));
    });
    shell.wake(Duration::ZERO);
    assert_eq!(permission_told(&waos), DENIED, "as WaOS welcomed it");

    let served = served.borrow_mut().take().expect("serving");
    served.tell_permission();
    shell.wake(Duration::ZERO);
    assert_eq!(permission_told(&waos), DENIED, "read again as WaOS asks");
    assert_eq!(notifications.times_asked(), 0, "the user is not asked");
}

fn click(id: &str) -> NotificationActivation {
    NotificationActivation {
        id: id.to_owned(),
        action: Some("answer".to_owned()),
        reply: Some("hi".to_owned()),
    }
}

/// The id of the click the window process sent WaOS next.
fn click_sent(waos: &waos_window_messages::os::macos::ReceiveRight) -> String {
    let message = sent(waos);
    let FromWindow::NotificationActivated(click) = message else {
        panic!("a click: {message:?}");
    };
    click.id
}

#[test]
fn the_click_that_launched_the_app_is_its_reason_and_the_rest_go_once_waos_welcomed_it() {
    let platform = TestPlatform::new().with_notifications(Rc::new(MockNotifications::new()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let clicks = Rc::new(RefCell::new(None));
    let mut shell = Shell::new(Rc::new(platform), |app| {
        *clicks.borrow_mut() = Some(NotificationClicks::listen(app));
    });
    shell.notification_activated(click("greeting"));
    shell.notification_activated(click("reminder"));
    shell.wake(Duration::ZERO);
    let clicks = clicks.borrow_mut().take().expect("listening");

    let LaunchReason::NotificationActivated(first) = clicks.launch_reason(Launcher::User) else {
        panic!("the user's click launched the app");
    };
    assert_eq!(
        (first.id.as_str(), first.reply.as_deref()),
        ("greeting", Some("hi"))
    );
    clicks.connected(connection);
    assert_eq!(
        click_sent(&waos),
        "reminder",
        "the one after it, once welcomed"
    );
    shell.notification_activated(click("later"));
    shell.wake(Duration::ZERO);
    assert_eq!(click_sent(&waos), "later", "as it comes, from then on");
}

#[test]
fn an_app_waos_launched_says_so_and_sends_a_click_once_welcomed() {
    let platform = TestPlatform::new().with_notifications(Rc::new(MockNotifications::new()));
    let (connection, waos) = connection();
    let clicks = Rc::new(RefCell::new(None));
    let mut shell = Shell::new(Rc::new(platform), |app| {
        *clicks.borrow_mut() = Some(NotificationClicks::listen(app));
    });
    shell.notification_activated(click("greeting"));
    shell.wake(Duration::ZERO);
    let clicks = clicks.borrow_mut().take().expect("listening");
    assert!(matches!(
        clicks.launch_reason(Launcher::Waos),
        LaunchReason::ProgramRunning
    ));
    clicks.connected(Rc::new(connection));
    assert_eq!(click_sent(&waos), "greeting");

    let alone = NotificationClicks(Rc::new(RefCell::new(Clicks::Collecting(VecDeque::new()))));
    assert!(
        matches!(
            alone.launch_reason(Launcher::User),
            LaunchReason::UserOpened
        ),
        "an app the user opened, with no click"
    );
}
