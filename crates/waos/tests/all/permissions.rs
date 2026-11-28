//! `examples/permissions` under the test shell, with the permissions WaOS hands an app, which the
//! test changes as the user would: the program reads where each permission stands, puts the question
//! about notifications to its user once however often it asks, keeps the answer, opens its
//! network page and reads the switch the user turned off there once the user leaves it, and
//! hears each change its user makes. Where the system keeps a permission of its own for the
//! program's notifications and denies it, asking reads denied and the program's stream of changes
//! hears it, the user's answer kept. With no grants, nothing is granted, no question is put and no
//! page opens.

use std::path::Path;
use std::sync::Arc;

use waos::{Answer, ProgramEnd, ProgramEnvironment, Question, Switch, WaOS};
use waos_capabilities::{DomainRule, NetworkGrant, Setting};
use waos_services::permissions::{Permissions, ProgramId};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// What the program prints first: where each permission stands at WaOS's defaults.
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
        .load(
            Path::new(waos_test_programs::PERMISSIONS),
            &waos_test_programs::code_of(waos_test_programs::PERMISSIONS),
        )
        .unwrap();
    let permissions = shell.permissions();
    let grants = permissions.of(&app());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants)),
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
        [(app().to_string(), Question::Notifications)],
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
        [(app().to_string(), Switch::Network)]
    );
    set_network(&permissions, false);
    output.wait_for("network changed: denied");
    assert!(
        !output.has("network after its settings: denied"),
        "not before the user leaves"
    );
    shell.prompts.leave_settings();
    output.wait_for("network after its settings: denied");

    let changed = permissions.change_app_settings(&app(), |own| {
        own.clipboard_write = Setting::Own(false);
    });
    changed.unwrap();
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
        .load(
            Path::new(waos_test_programs::PERMISSIONS),
            &waos_test_programs::code_of(waos_test_programs::PERMISSIONS),
        )
        .unwrap();
    let permissions = shell.permissions();
    let grants = permissions.of(&app());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants)),
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
        .load(
            Path::new(waos_test_programs::PERMISSIONS),
            &waos_test_programs::code_of(waos_test_programs::PERMISSIONS),
        )
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
    output.wait_for("network after its settings: denied");
    assert!(
        shell.prompts.questions().is_empty() && shell.prompts.settings_opened().is_empty(),
        "nothing to ask and no page to open: nothing keeps its settings"
    );
    running.stop();
    within(running.ended());
}

/// How many of `lines` are `line`.
fn count(lines: &[String], line: &str) -> usize {
    lines.iter().filter(|printed| *printed == line).count()
}

/// The id the program runs as, which its permissions are kept by.
fn app() -> ProgramId {
    "dev.waos.test.permissions".parse().unwrap()
}

/// The user sets the app's own network switch, on or off.
fn set_network(permissions: &Permissions, on: bool) {
    let network = NetworkGrant {
        on,
        domains: DomainRule::Any,
    };
    let changed = permissions.change_app_settings(&app(), |own| {
        own.network = Setting::Own(network);
    });
    changed.unwrap();
}
