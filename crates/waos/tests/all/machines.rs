//! `examples/machines` under the test shell, given this computer as a machine whose commands run
//! in a login environment the test gives, never the user's own shell, with grants of the test's own
//! that keep the machines picked as an app keeps them: a machine its user picks is the program's
//! and kept among its grants, and the program reads what it runs and its user's login shell, as
//! the provider says them, runs `/bin/echo` with pipes and reads what it
//! printed, reads a file in a new folder from the machine's root by its absolute path, through a
//! symlink holding its absolute path and from above `/`, as any program on the machine does, runs
//! `/bin/sh` in a terminal that echoes what is typed and takes a new size, and ends a command with
//! `terminate` and another with `interrupt`, and ends while a command it started with pipes and
//! another in a terminal run, which leaves neither behind; a machine granted before needs no pick; a pick cancelled, or of a machine no provider
//! offers, gives nothing and keeps nothing; a machine its user set as the picker's answer is the
//! pick, kept, with nothing shown, and one set that no provider offers is none, with nothing shown
//! in its place; and a program given no grants and a shell with no
//! picker holds no machine, nor one given no machines at all.

use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use waos::{Answer, ProgramEnvironment, ProgramGrants, Question, RunningProgram, WaOS};
use waos_capabilities::{
    Architecture, ChangedFuture, DomainRule, FileSystemGrant, Machines, NetworkGrant,
    OperatingSystem,
};
use waos_local_processes::{LocalMachines, LoginEnvironment, THIS_COMPUTER};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// What the program says of a machine picked and acted on, in order.
#[cfg(unix)]
const ACTED_ON: [&str; 13] = [
    "granted after: [This computer]",
    "echo: hello",
    "echo ended: exited 0",
    "root read: from the machine",
    "root read through an absolute symlink: from the machine",
    "root read from above /: from the machine",
    "terminal: 42",
    "terminal resized: 30 100",
    "terminal ended: exited 4",
    "signaled: signaled TERM",
    "interrupted: signaled INT",
    "left running: sleep 6011, sleep 6012",
    "done",
];

// `examples/machines` runs Unix's commands, `/bin/echo` and `/bin/sh`; Windows' are tested in
// `waos-local-processes`.
#[cfg(unix)]
#[test]
fn a_machine_picked_is_kept_and_acted_on_with_pipes_a_terminal_signals_and_its_root() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.machine_picker.answer_with(Some(THIS_COMPUTER));
    let grants = Arc::new(MachineGrants::default());
    let output = Arc::new(RecordedOutput::default());
    let running = run(&engine, Some(&grants), vec![this_computer()], &output);
    output.wait_for("granted: []");
    output.wait_for("picked: This computer");
    output.wait_for(&what_this_computer_runs());
    output.wait_for(&this_computers_login_shell());
    for line in ACTED_ON {
        output.wait_for(line);
    }
    assert_eq!(
        shell.machine_picker.asked(),
        [(
            "machines".to_owned(),
            vec![waos::MachineChoice {
                id: THIS_COMPUTER.to_owned(),
                name: "This computer".to_owned(),
            }]
        )],
        "the user was shown this computer, once"
    );
    assert_eq!(grants.machines(), [THIS_COMPUTER], "kept among its grants");
    within(running.ended());
    for left_running in ["sleep 6011", "sleep 6012"] {
        until_gone(left_running);
    }
}

/// Waits until no process of this computer runs `command_line`, as its program's end ends it.
#[cfg(unix)]
fn until_gone(command_line: &str) {
    wait_until(&format!("{command_line} ends with its program"), || {
        let found = std::process::Command::new("pgrep")
            .args(["-f", command_line])
            .output()
            .expect("pgrep runs");
        found.stdout.is_empty()
    });
}

// `examples/machines` runs Unix's commands, `/bin/echo` and `/bin/sh`; Windows' are tested in
// `waos-local-processes`.
#[cfg(unix)]
#[test]
fn a_machine_granted_before_needs_no_pick() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let grants = Arc::new(MachineGrants::default());
    grants.keep_machine(THIS_COMPUTER);
    let output = Arc::new(RecordedOutput::default());
    let running = run(&engine, Some(&grants), vec![this_computer()], &output);
    output.wait_for("granted: [This computer]");
    output.wait_for("done");
    assert!(shell.machine_picker.asked().is_empty(), "nobody asked");
    within(running.ended());
}

#[test]
fn a_pick_cancelled_or_of_a_machine_not_offered_gives_nothing_and_keeps_nothing() {
    for answer in [None, Some("build.example.com")] {
        let shell = TestShell::new();
        let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
        shell.machine_picker.answer_with(answer);
        let grants = Arc::new(MachineGrants::default());
        let output = Arc::new(RecordedOutput::default());
        let running = run(&engine, Some(&grants), vec![this_computer()], &output);
        within(running.ended());
        assert_eq!(
            output.lines(),
            ["granted: []", "picked: none"],
            "{answer:?}"
        );
        assert!(grants.machines().is_empty(), "{answer:?}");
    }
}

#[test]
fn a_machine_set_as_the_pickers_answer_is_picked_and_kept_unasked_and_one_not_offered_is_none() {
    for (set, kept) in [
        (THIS_COMPUTER, vec![THIS_COMPUTER]),
        ("build.example.com", Vec::new()),
    ] {
        let shell = TestShell::new();
        let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
        shell.machine_picker.answer_with(Some(THIS_COMPUTER));
        let grants = Arc::new(MachineGrants::answering_the_picker_with(set));
        let output = Arc::new(RecordedOutput::default());
        let running = run(&engine, Some(&grants), vec![this_computer()], &output);
        let picked = if kept.is_empty() {
            "picked: none"
        } else {
            "picked: This computer"
        };
        output.wait_for(picked);
        running.stop();
        within(running.ended());
        assert!(
            shell.machine_picker.asked().is_empty(),
            "nothing shown for {set}"
        );
        assert_eq!(grants.machines(), kept, "{set}");
    }
}

/// What the program says this computer runs, as the provider says it.
fn what_this_computer_runs() -> String {
    let platform = this_computer()
        .machine(THIS_COMPUTER)
        .expect("this computer")
        .platform();
    let os = match platform.os {
        OperatingSystem::Macos => "macos".to_owned(),
        OperatingSystem::Linux => "linux".to_owned(),
        OperatingSystem::Windows => "windows".to_owned(),
        OperatingSystem::Other(name) => name,
    };
    let arch = match platform.arch {
        Architecture::Aarch64 => "aarch64".to_owned(),
        Architecture::X86_64 => "x86-64".to_owned(),
        Architecture::Other(name) => name,
    };
    format!("platform: {os} {arch}")
}

/// What the program says this computer's login shell is, as the provider says it.
fn this_computers_login_shell() -> String {
    let shell = this_computer()
        .machine(THIS_COMPUTER)
        .expect("this computer")
        .login_shell();
    format!("login shell: {}", shell.as_deref().unwrap_or("none"))
}

#[test]
fn a_program_given_no_grants_and_a_shell_with_no_picker_holds_no_machine() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.machine_picker = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = run(&engine, None, vec![this_computer()], &output);
    within(running.ended());
    assert_eq!(output.lines(), ["granted: []", "picked: none"]);
}

#[test]
fn a_program_given_no_machines_has_none() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.machine_picker.answer_with(Some(THIS_COMPUTER));
    let output = Arc::new(RecordedOutput::default());
    let grants = Arc::new(MachineGrants::default());
    let running = run(&engine, Some(&grants), Vec::new(), &output);
    within(running.ended());
    assert_eq!(output.lines(), ["machines: none"]);
    assert!(shell.machine_picker.asked().is_empty());
}

#[test]
fn a_waiting_machine_picker_is_answered_by_setting_its_row_and_drops_its_prompt() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.machine_picker.hold_picks();
    let grants = Arc::new(MachineGrants::default());
    let output = Arc::new(RecordedOutput::default());
    let running = run(&engine, Some(&grants), vec![this_computer()], &output);

    wait_until("machine picker is waiting", || {
        !shell.machine_picker.asked().is_empty()
    });
    assert!(!shell.machine_picker.held_cancelled(0));

    grants.set_machine_answer(THIS_COMPUTER);
    wait_until("machine picker future is dropped", || {
        shell.machine_picker.held_cancelled(0)
    });
    output.wait_for("picked: This computer");
    running.stop();
    within(running.ended());
}

/// This computer's machines, its commands run with `PATH` and `HOME` alone.
fn this_computer() -> Arc<dyn Machines> {
    let home = std::env::temp_dir();
    Arc::new(LocalMachines::new(LoginEnvironment::given(vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), home.into_os_string()),
    ])))
}

/// Runs `examples/machines` with `grants`, where it has any, and `machines`, its output into
/// `output`.
fn run(
    engine: &WaOS,
    grants: Option<&Arc<MachineGrants>>,
    machines: Vec<Arc<dyn Machines>>,
    output: &Arc<RecordedOutput>,
) -> RunningProgram {
    let environment = ProgramEnvironment {
        grants: grants.map(|grants| Arc::clone(grants) as Arc<dyn ProgramGrants>),
        machines,
        ..printing_to(output)
    };
    let program = engine
        .load(
            Path::new(waos_test_programs::MACHINES),
            &waos_test_programs::code_of(waos_test_programs::MACHINES),
        )
        .unwrap();
    engine.run(&program, environment).unwrap()
}

/// Grants that give nothing but the machines picked, kept as an app keeps them, and the
/// picker's answer where a test set one.
#[derive(Default)]
struct MachineGrants {
    machines: Mutex<Vec<String>>,
    answer: Mutex<Option<String>>,
    changed: Arc<Notify>,
}

impl MachineGrants {
    /// Grants whose picker of machines answers `id`, unasked.
    fn answering_the_picker_with(id: &str) -> MachineGrants {
        MachineGrants {
            answer: Mutex::new(Some(id.to_owned())),
            ..MachineGrants::default()
        }
    }

    fn set_machine_answer(&self, id: &str) {
        *self.answer.lock().unwrap() = Some(id.to_owned());
        self.changed.notify_waiters();
    }
}

impl ProgramGrants for MachineGrants {
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

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn machines(&self) -> Vec<String> {
        self.machines.lock().unwrap().clone()
    }

    fn keep_machine(&self, id: &str) {
        let mut machines = self.machines.lock().unwrap();
        if !machines.iter().any(|kept| kept == id) {
            machines.push(id.to_owned());
        }
    }

    fn machine_answer(&self) -> Option<String> {
        self.answer.lock().unwrap().clone()
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }
}
