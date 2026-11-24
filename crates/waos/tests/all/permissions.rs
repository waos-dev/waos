//! `examples/permissions` under the test shell, with grants of the test's own that the test
//! changes as the user would: the program reads where each permission stands, puts the question
//! about notifications to its user once however often it asks, keeps the answer, opens its
//! network page and reads the switch the user turned off there once the user leaves it, and
//! hears each change its user makes. Where the system keeps a permission of its own for the
//! program's notifications and denies it, asking reads denied and the program's stream of changes
//! hears it, the user's answer kept. With no grants, nothing is granted and no question is put.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use waos::{Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, Question, Switch, WaOS};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// What the program prints first: where each permission stands under [`TestGrants::new`].
const FIRST_STATES: [&str; 7] = [
    "network: granted",
    "local-network: denied",
    "clipboard-read: granted",
    "clipboard-write: granted",
    "notifications: not asked",
    "camera: not asked",
    "microphone: not asked",
];

#[test]
fn a_program_reads_its_permissions_asks_once_and_hears_each_change() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::PERMISSIONS))
        .unwrap();
    let grants = Arc::new(TestGrants::new());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();
    for state in FIRST_STATES {
        output.wait_for(state);
    }
    assert!(
        !output
            .lines()
            .iter()
            .any(|line| line.starts_with("processes")),
        "no permission for processes: machines are picked"
    );

    wait_until("the question is put", || {
        !shell.prompts.questions().is_empty()
    });
    shell.prompts.answer_questions(Some(Answer::Granted));
    output.wait_for("notifications changed: granted");
    output.wait_until("both asks answered", |lines| {
        count(lines, "notifications asked: granted") == 2
    });
    assert_eq!(
        shell.prompts.questions(),
        [("permissions".to_owned(), Question::Notifications)],
        "put once, though asked twice"
    );
    assert_eq!(
        grants.answer(Question::Notifications),
        Some(Answer::Granted),
        "kept"
    );

    wait_until("the network page opens", || {
        !shell.prompts.settings_opened().is_empty()
    });
    assert_eq!(
        shell.prompts.settings_opened(),
        [("permissions".to_owned(), Switch::Network)]
    );
    grants.set_network(false);
    output.wait_for("network changed: denied");
    assert!(
        !output.has("network after its settings: denied"),
        "not before the user leaves"
    );
    shell.prompts.leave_settings();
    output.wait_for("network after its settings: denied");

    grants.set_clipboard_write(false);
    output.wait_for("clipboard-write changed: denied");
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

#[test]
fn notifications_the_user_allowed_read_denied_once_the_system_asked_after_denies_them() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.notifications.keep_permissions();
    shell.notifications.refuse(true);
    let program = engine
        .load(Path::new(waos_test_programs::PERMISSIONS))
        .unwrap();
    let grants = Arc::new(TestGrants::new());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();
    output.wait_for("notifications: not asked");
    wait_until("the question is put", || {
        !shell.prompts.questions().is_empty()
    });
    assert!(
        shell.notifications.asked().is_empty(),
        "the system is not asked before the user allowed them in WaOS"
    );
    shell.prompts.answer_questions(Some(Answer::Granted));
    output.wait_until("both asks answered", |lines| {
        count(lines, "notifications asked: denied") == 2
    });
    output.wait_for("notifications changed: denied");
    assert!(
        !shell.notifications.asked().is_empty(),
        "the system is asked once the user allowed them"
    );
    assert_eq!(
        grants.answer(Question::Notifications),
        Some(Answer::Granted),
        "the user's answer is kept while the system denies"
    );
    running.stop();
}

#[test]
fn a_program_given_no_grants_is_granted_nothing_and_asks_nobody() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::PERMISSIONS))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    for name in [
        "network",
        "local-network",
        "clipboard-read",
        "clipboard-write",
    ] {
        output.wait_for(&format!("{name}: denied"));
    }
    for name in ["notifications", "camera", "microphone"] {
        output.wait_for(&format!("{name}: denied"));
    }
    output.wait_for("notifications asked: denied");
    wait_until("the network page opens", || {
        !shell.prompts.settings_opened().is_empty()
    });
    shell.prompts.leave_settings();
    output.wait_for("network after its settings: denied");
    assert!(
        shell.prompts.questions().is_empty(),
        "nothing to ask: nothing can be granted"
    );
    running.stop();
    within(running.ended());
}

#[test]
fn a_waiting_permission_question_is_answered_by_setting_its_row_and_drops_its_prompt() {
    for (set_to, expected_line) in [
        (Answer::Granted, "notifications asked: granted"),
        (Answer::Denied, "notifications asked: denied"),
    ] {
        let shell = TestShell::new();
        let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
        let program = engine
            .load(Path::new(waos_test_programs::PERMISSIONS))
            .unwrap();
        let grants = Arc::new(TestGrants::new());
        let output = Arc::new(RecordedOutput::default());
        let environment = ProgramEnvironment {
            grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
            ..printing_to(&output)
        };
        let running = engine.run(&program, environment).unwrap();
        wait_until("the question is put", || {
            !shell.prompts.questions().is_empty()
        });
        assert!(!shell.prompts.question_cancelled(0));

        // Unrelated setting changes leave the question waiting and its prompt intact.
        grants.set_network(false);
        let reads = grants.answer_reads();
        grants.set_camera(Answer::Granted);
        wait_until("notifications answer re-read", || {
            grants.answer_reads() > reads
        });
        assert!(
            !shell.prompts.question_cancelled(0),
            "unrelated setting change left prompt waiting"
        );
        assert!(
            !output.has("notifications asked: granted")
                && !output.has("notifications asked: denied"),
            "still waiting"
        );

        grants.set_notifications(set_to);
        wait_until("the prompt future is dropped", || {
            shell.prompts.question_cancelled(0)
        });
        output.wait_until("both asks answered", |lines| {
            count(lines, expected_line) == 2
        });

        running.stop();
        within(running.ended());
    }
}

/// How many of `lines` are `line`.
fn count(lines: &[String], line: &str) -> usize {
    lines.iter().filter(|printed| *printed == line).count()
}

/// Grants of the test's own: the network and the clipboard on, nothing mounted,
/// no question answered, each change heard by whoever waits for one.
struct TestGrants {
    values: Mutex<Values>,
    answer_reads: AtomicUsize,
    changed: Arc<Notify>,
}

struct Values {
    network: bool,
    clipboard_write: bool,
    notifications: Option<Answer>,
    camera: Option<Answer>,
}

impl TestGrants {
    fn new() -> TestGrants {
        TestGrants {
            values: Mutex::new(Values {
                network: true,
                clipboard_write: true,
                notifications: None,
                camera: None,
            }),
            answer_reads: AtomicUsize::new(0),
            changed: Arc::new(Notify::new()),
        }
    }

    fn answer_reads(&self) -> usize {
        self.answer_reads.load(Ordering::SeqCst)
    }

    /// The user turns the network on or off.
    fn set_network(&self, on: bool) {
        self.values.lock().unwrap().network = on;
        self.changed.notify_waiters();
    }

    /// The user sets the notifications row.
    fn set_notifications(&self, answer: Answer) {
        self.values.lock().unwrap().notifications = Some(answer);
        self.changed.notify_waiters();
    }

    /// The user sets the camera row.
    fn set_camera(&self, answer: Answer) {
        self.values.lock().unwrap().camera = Some(answer);
        self.changed.notify_waiters();
    }

    /// The user turns writing the clipboard on or off.
    fn set_clipboard_write(&self, on: bool) {
        self.values.lock().unwrap().clipboard_write = on;
        self.changed.notify_waiters();
    }
}

impl ProgramGrants for TestGrants {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: self.values.lock().unwrap().network,
            domains: DomainRule::Any,
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        true
    }

    fn clipboard_write(&self) -> bool {
        self.values.lock().unwrap().clipboard_write
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        if question == Question::Notifications {
            self.answer_reads.fetch_add(1, Ordering::SeqCst);
        }
        let values = self.values.lock().unwrap();
        match question {
            Question::Notifications => values.notifications,
            Question::Camera => values.camera,
            Question::Microphone => None,
        }
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        assert_eq!(
            question,
            Question::Notifications,
            "the program asks only this"
        );
        self.values.lock().unwrap().notifications = Some(answer);
        self.changed.notify_waiters();
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        None
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {
        panic!("the program opens no link");
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }
}
