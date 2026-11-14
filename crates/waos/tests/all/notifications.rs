//! `examples/notifications` under the test shell, with grants of the test's own that keep the
//! answer to the notifications question as an app keeps it: a post without the user's permission
//! is denied and asks nothing, and once the program has asked and its user allowed it, its posts
//! are shown, a post of the same id replacing the one before, with a button and an action answered
//! by typing, a reminder is scheduled at its time and a removal reaches the shell; with no grants,
//! every post is denied and nobody is asked. Ids are each program's own: two programs' same id are
//! two notifications. What the user does to a notification reaches every stream of the program's
//! activations, and one made before the program listens, as the click that starts it, waits for
//! its first; an activation thaws a frozen program until it has read it, on time the test moves
//! (`EngineTime::Manual`), the program frozen again once it reads on (`RunningProgram::frozen`). A
//! shell without replies or scheduling offers neither, and one without notifications gives the
//! program none. A system that keeps a permission of its own for each program, as macOS does for
//! each program's app, is asked after WaOS, only once the user allowed the program notifications
//! there, and its refusal reads `denied`, the user's answer kept, every post still reaching the
//! shell, which answers it `denied`; a system that keeps none and denies a post leaves the
//! permission as the user set it, since the engine never reads a refusal from a post's answer.

use std::future::pending;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use waos::bindings::wasdk::platform::notifications::Activation;
use waos::{
    Answer, FreezePolicy, ManualTime, NotificationAction, ProgramEnvironment, ProgramGrants,
    ProgramOutput, Question, RunningProgram, WaOS,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

use crate::shell::{self, Lines, TestShell, resolved, wait_until, wait_until_frozen};

/// The engine's tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

#[test]
fn a_post_is_denied_without_asking_until_the_user_allows_it_then_shown_scheduled_and_removed() {
    let (engine, shell, _arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let running = run(
        &engine,
        "notifications",
        Arc::new(NotificationGrants::not_asked()),
        &lines,
    );
    lines.wait_for("permission: not asked");
    lines.wait_for("post greeting: denied");
    allow_when_asked(&shell);
    for line in [
        "permission asked: granted",
        "post greeting again: ok",
        "post greeting with a reply: ok",
        "schedule reminder: ok",
        "remove old: done",
        "listening for activations",
    ] {
        lines.wait_for(line);
    }
    assert_eq!(shell.questions().len(), 1, "the denied post asked nothing");

    let shown = shell.notifications().shown();
    assert_eq!(shown.len(), 1, "each post replaced the one before");
    let (program, greeting) = &shown[0];
    assert_eq!(program, "notifications");
    assert_eq!(
        (greeting.id.as_str(), greeting.title.as_str()),
        ("greeting", "Hello")
    );
    assert_eq!(
        greeting.body.as_deref(),
        Some("From examples/notifications")
    );
    assert_eq!(
        greeting.actions,
        [
            action("later", "Later", None),
            action("answer", "Answer", Some("Say something")),
        ]
    );
    let scheduled = shell.notifications().scheduled();
    assert_eq!(scheduled.len(), 1);
    assert_eq!(scheduled[0].notification.id, "reminder");
    assert!(scheduled[0].notification.silent);
    assert_eq!(
        scheduled[0].at,
        UNIX_EPOCH + Duration::from_secs(4_102_444_800)
    );
    assert_eq!(
        shell.notifications().removed(),
        [("notifications".to_owned(), "old".to_owned())]
    );
    stop(&running);
}

#[test]
fn a_program_given_no_grants_is_denied_every_post_and_nobody_is_asked() {
    let (engine, shell, _arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(&engine, waos_test_programs::NOTIFICATIONS);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    for line in [
        "permission: denied",
        "post greeting: denied",
        "permission asked: denied",
        "post greeting again: denied",
        "post greeting with a reply: denied",
        "schedule reminder: denied",
        "listening for activations",
    ] {
        lines.wait_for(line);
    }
    assert!(shell.questions().is_empty());
    assert!(shell.notifications().shown().is_empty());
    assert!(shell.notifications().scheduled().is_empty());
    stop(&running);
}

#[test]
fn a_system_with_a_permission_per_program_is_asked_after_waos_and_its_refusal_reads_denied() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.notifications().keep_permissions();
    shell.notifications().refuse(true);
    let lines = Arc::new(Lines::default());
    let grants = Arc::new(NotificationGrants::not_asked());
    let running = run(&engine, "notifications", Arc::clone(&grants), &lines);
    lines.wait_for("permission: not asked");
    lines.wait_for("post greeting: denied");
    wait_until("the program asks", || !shell.questions().is_empty());
    assert!(
        shell.notifications().asked().is_empty(),
        "the system is not asked before WaOS's question is answered"
    );
    shell.answer_questions(Some(Answer::Granted));
    for line in [
        "permission asked: denied",
        "post greeting again: denied",
        "post greeting with a reply: denied",
        "schedule reminder: denied",
        "listening for activations",
    ] {
        lines.wait_for(line);
    }
    assert_eq!(
        shell.notifications().asked(),
        ["notifications"],
        "asked once WaOS's answer was granted"
    );
    assert_eq!(
        grants.answer(Question::Notifications),
        Some(Answer::Granted),
        "the user's answer is kept"
    );
    assert_eq!(
        refused_ids(&shell),
        ["greeting", "greeting", "reminder"],
        "every post after WaOS's grant reached the shell, which says what the system does"
    );
    stop(&running);
}

#[test]
fn a_system_that_refuses_the_program_reads_as_denied_and_the_users_answer_is_kept() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.notifications().keep_permissions();
    shell.notifications().refuse(true);
    let lines = Arc::new(Lines::default());
    let grants = Arc::new(NotificationGrants::granted());
    let running = run(&engine, "notifications", Arc::clone(&grants), &lines);
    for line in [
        "permission: granted",
        "post greeting: denied",
        "permission asked: denied",
        "post greeting again: denied",
        "post greeting with a reply: denied",
        "schedule reminder: denied",
        "listening for activations",
    ] {
        lines.wait_for(line);
    }
    assert_eq!(
        grants.answer(Question::Notifications),
        Some(Answer::Granted),
        "the user's answer is kept"
    );
    assert!(
        shell.questions().is_empty(),
        "nobody was asked in WaOS again"
    );
    assert_eq!(
        shell.notifications().asked(),
        ["notifications"],
        "the system is asked although WaOS's question was answered before"
    );
    assert_eq!(
        refused_ids(&shell),
        ["greeting", "greeting", "greeting", "reminder"],
        "every post reached the shell, which alone can say the system lets them again"
    );
    stop(&running);
}

#[test]
fn a_post_a_system_without_a_permission_per_program_denies_leaves_the_permission_as_the_user_set_it()
 {
    let (engine, shell, _arrivals) = shell::engine();
    shell.notifications().refuse(true);
    let lines = Arc::new(Lines::default());
    let grants = Arc::new(NotificationGrants::granted());
    let running = run(&engine, "notifications", Arc::clone(&grants), &lines);
    for line in [
        "permission: granted",
        "post greeting: denied",
        "permission asked: granted",
        "post greeting again: denied",
        "listening for activations",
    ] {
        lines.wait_for(line);
    }
    assert!(
        shell.notifications().asked().is_empty(),
        "a system with no permission per program has no question of its own"
    );
    stop(&running);
}

/// The ids of the posts and schedules the system refused, in order.
fn refused_ids(shell: &TestShell) -> Vec<String> {
    shell
        .notifications()
        .refused()
        .into_iter()
        .map(|(_program, id)| id)
        .collect()
}

#[test]
fn a_click_made_before_the_program_listens_reaches_its_first_stream_and_later_ones_every_stream() {
    let (engine, shell, _arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let running = run(
        &engine,
        "notifications",
        Arc::new(NotificationGrants::not_asked()),
        &lines,
    );
    wait_until("the program asks, not listening yet", || {
        !shell.questions().is_empty()
    });
    running.notification_activated(activation("later", None));
    allow_when_asked(&shell);
    lines.wait_for("activated: greeting Some(\"later\") None");

    running.notification_activated(activation("answer", Some("on my way")));
    lines.wait_for("activated: greeting Some(\"answer\") Some(\"on my way\")");
    // The second stream hears in order, so it says the click that waited before this, if at all.
    lines.wait_for("heard again: greeting Some(\"answer\")");
    assert!(
        !lines.has("heard again: greeting Some(\"later\")"),
        "the click that waited went to the first stream alone"
    );
    stop(&running);
}

#[test]
fn an_activation_thaws_a_frozen_program_until_it_has_read_it() {
    let time = ManualTime::new();
    let only_when_suspended = FreezePolicy { after_hidden: None };
    let (engine, _shell, _arrivals) = shell::engine_on(&time, only_when_suspended);
    let lines = Arc::new(Lines::default());
    let running = run(
        &engine,
        "notifications",
        Arc::new(NotificationGrants::granted()),
        &lines,
    );
    lines.wait_for("listening for activations");

    resolved(engine.app_suspending());
    wait_until_frozen(&running);
    // A tick past the moment it last ran: woken without a thaw, it would stop at its next epoch
    // check, before it could say what it heard.
    time.elapse(TICK);
    running.notification_activated(activation("later", None));
    lines.wait_for("activated: greeting Some(\"later\") None");
    lines.wait_for("heard again: greeting Some(\"later\")");
    wait_until_frozen(&running);

    engine.app_resumed();
    stop(&running);
}

#[test]
fn ids_are_each_programs_own() {
    let (engine, shell, _arrivals) = shell::engine();
    let (first_lines, second_lines) = (Arc::new(Lines::default()), Arc::new(Lines::default()));
    let first = run(
        &engine,
        "first",
        Arc::new(NotificationGrants::granted()),
        &first_lines,
    );
    let second = run(
        &engine,
        "second",
        Arc::new(NotificationGrants::granted()),
        &second_lines,
    );
    first_lines.wait_for("listening for activations");
    second_lines.wait_for("listening for activations");

    let mut shown: Vec<(String, String)> = shell
        .notifications()
        .shown()
        .into_iter()
        .map(|(program, notification)| (program, notification.id))
        .collect();
    shown.sort();
    let of = |program: &str, id: &str| (program.to_owned(), id.to_owned());
    assert_eq!(
        shown,
        [of("first", "greeting"), of("second", "greeting")],
        "two notifications of one id"
    );
    let mut removed = shell.notifications().removed();
    removed.sort();
    assert_eq!(removed, [of("first", "old"), of("second", "old")]);
    stop(&first);
    stop(&second);
}

#[test]
fn a_shell_without_replies_or_scheduling_offers_neither() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.notifications().without_replies_or_scheduling();
    let lines = Arc::new(Lines::default());
    let running = run(
        &engine,
        "notifications",
        Arc::new(NotificationGrants::granted()),
        &lines,
    );
    lines.wait_for("replies: none");
    lines.wait_for("scheduling: none");
    lines.wait_for("listening for activations");
    assert!(shell.notifications().scheduled().is_empty());
    stop(&running);
}

#[test]
fn a_shell_without_notifications_gives_the_program_none() {
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.notifications = None;
    });
    let lines = Arc::new(Lines::default());
    let running = run(
        &engine,
        "notifications",
        Arc::new(NotificationGrants::granted()),
        &lines,
    );
    shell::wait_for_end(&running);
    assert_eq!(lines.all(), ["notifications: none"]);
}

/// Runs `examples/notifications`, named `name`, with `grants`, its output into `lines`.
fn run(
    engine: &WaOS,
    name: &str,
    grants: Arc<NotificationGrants>,
    lines: &Arc<Lines>,
) -> RunningProgram {
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(lines) as Arc<dyn ProgramOutput>),
        grants: Some(grants as Arc<dyn ProgramGrants>),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(engine, waos_test_programs::NOTIFICATIONS).named(name);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    running
}

/// Waits until the program asks its user for notifications, and allows them.
fn allow_when_asked(shell: &TestShell) {
    wait_until("the program asks", || !shell.questions().is_empty());
    assert_eq!(
        shell.questions(),
        [("notifications".to_owned(), Question::Notifications)]
    );
    shell.answer_questions(Some(Answer::Granted));
}

/// The user chose the greeting's `action`, typing `reply`.
fn activation(action: &str, reply: Option<&str>) -> Activation {
    Activation {
        id: "greeting".to_owned(),
        action: Some(action.to_owned()),
        reply: reply.map(str::to_owned),
    }
}

fn action(id: &str, label: &str, reply_placeholder: Option<&str>) -> NotificationAction {
    NotificationAction {
        id: id.to_owned(),
        label: label.to_owned(),
        reply_placeholder: reply_placeholder.map(str::to_owned),
    }
}

fn stop(running: &RunningProgram) {
    running.stop();
    shell::wait_for_end(running);
}

/// Grants that give nothing but notifications, as the user answered their question, kept as an
/// app keeps it.
struct NotificationGrants {
    notifications: Mutex<Option<Answer>>,
}

impl NotificationGrants {
    fn not_asked() -> NotificationGrants {
        NotificationGrants {
            notifications: Mutex::new(None),
        }
    }

    fn granted() -> NotificationGrants {
        NotificationGrants {
            notifications: Mutex::new(Some(Answer::Granted)),
        }
    }
}

impl ProgramGrants for NotificationGrants {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Any,
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        match question {
            Question::Notifications => *self.notifications.lock().unwrap(),
            Question::Camera | Question::Microphone => Some(Answer::Denied),
        }
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        assert_eq!(
            question,
            Question::Notifications,
            "the program asks only this"
        );
        *self.notifications.lock().unwrap() = Some(answer);
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
