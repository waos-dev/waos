//! `examples/permissions` under the test shell, with grants of the test's own that the test
//! changes as the user would: the program reads where each permission stands, puts the question
//! about notifications to its user once however often it asks, keeps the answer, opens its
//! network page and reads the switch the user turned off there once the user leaves it, and
//! hears each change, the system's refusal of its notifications and its letting them again among
//! them, as the shell reports what the system says without a post. With no grants, nothing is
//! granted and no question is put.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::executor::block_on;
use waos::{
    Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, Question, Switch,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

use crate::shell::{self, Lines, WITHIN};

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
    let (engine, shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::PERMISSIONS);
    let grants = Arc::new(TestGrants::new());
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        ..ProgramEnvironment::default()
    };
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    for state in FIRST_STATES {
        lines.wait_for(state);
    }
    assert!(
        !lines.all().iter().any(|line| line.starts_with("processes")),
        "no permission for processes: machines are picked"
    );

    wait_until("the question is put", || !shell.questions().is_empty());
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        shell.questions(),
        [("permissions".to_owned(), Question::Notifications)],
        "put once, though asked twice"
    );
    shell.answer_questions(Some(Answer::Granted));
    lines.wait_for("notifications changed: granted");
    assert_eq!(
        lines.count("notifications asked: granted"),
        2,
        "{:#?}",
        lines.all()
    );
    assert_eq!(
        grants.answer(Question::Notifications),
        Some(Answer::Granted),
        "kept"
    );

    running.system_notifications_refused(true);
    lines.wait_for("notifications changed: denied");
    assert_eq!(
        grants.answer(Question::Notifications),
        Some(Answer::Granted),
        "the user's answer is kept while the system refuses"
    );
    running.system_notifications_refused(false);
    wait_until("the system lets them again", || {
        lines.count("notifications changed: granted") == 2
    });

    wait_until("the network page opens", || {
        !shell.settings_opened().is_empty()
    });
    assert_eq!(
        shell.settings_opened(),
        [("permissions".to_owned(), Switch::Network)]
    );
    grants.set_network(false);
    lines.wait_for("network changed: denied");
    assert!(
        !lines.has("network after its settings: denied"),
        "not before the user leaves"
    );
    shell.leave_settings();
    lines.wait_for("network after its settings: denied");

    grants.set_clipboard_write(false);
    lines.wait_for("clipboard-write changed: denied");
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

#[test]
fn a_program_given_no_grants_is_granted_nothing_and_asks_nobody() {
    let (engine, shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::PERMISSIONS);
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    for name in [
        "network",
        "local-network",
        "clipboard-read",
        "clipboard-write",
    ] {
        lines.wait_for(&format!("{name}: denied"));
    }
    for name in ["notifications", "camera", "microphone"] {
        lines.wait_for(&format!("{name}: denied"));
    }
    lines.wait_for("notifications asked: denied");
    wait_until("the network page opens", || {
        !shell.settings_opened().is_empty()
    });
    shell.leave_settings();
    lines.wait_for("network after its settings: denied");
    assert!(
        shell.questions().is_empty(),
        "nothing to ask: nothing can be granted"
    );
    running.stop();
    block_on(running.ended());
}

/// Waits until `done`, failing after [`WITHIN`].
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Grants of the test's own: the network and the clipboard on, nothing mounted,
/// no question answered, each change heard by whoever waits for one.
struct TestGrants {
    values: Mutex<Values>,
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
}

struct Values {
    network: bool,
    clipboard_write: bool,
    notifications: Option<Answer>,
}

impl TestGrants {
    fn new() -> TestGrants {
        TestGrants {
            values: Mutex::new(Values {
                network: true,
                clipboard_write: true,
                notifications: None,
            }),
            waiting: Mutex::default(),
        }
    }

    /// The user turns the network on or off.
    fn set_network(&self, on: bool) {
        self.values.lock().unwrap().network = on;
        self.changed_now();
    }

    /// The user turns writing the clipboard on or off.
    fn set_clipboard_write(&self, on: bool) {
        self.values.lock().unwrap().clipboard_write = on;
        self.changed_now();
    }

    fn changed_now(&self) {
        for waiting in self.waiting.lock().unwrap().drain(..) {
            let _ = waiting.send(());
        }
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
        match question {
            Question::Notifications => self.values.lock().unwrap().notifications,
            Question::Camera | Question::Microphone => None,
        }
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        assert_eq!(
            question,
            Question::Notifications,
            "the program asks only this"
        );
        self.values.lock().unwrap().notifications = Some(answer);
        self.changed_now();
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        None
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {
        panic!("the program opens no link");
    }

    fn changed(&self) -> ChangedFuture {
        let (changed, heard) = oneshot::channel();
        self.waiting.lock().unwrap().push(changed);
        Box::pin(async move {
            let _ = heard.await;
        })
    }
}
