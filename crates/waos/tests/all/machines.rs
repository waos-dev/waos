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

use std::future::pending;
use std::sync::{Arc, Mutex};

use waos::{
    Answer, ProgramEnvironment, ProgramGrants, ProgramOutput, Question, RunningProgram, WaOS,
};
use waos_capabilities::{
    Architecture, ChangedFuture, DomainRule, FileSystemGrant, Machines, NetworkGrant,
    OperatingSystem,
};
use waos_local_processes::{LocalMachines, LoginEnvironment, THIS_COMPUTER};

use crate::shell;
use crate::shell::Lines;

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
    let (engine, shell, _arrivals) = shell::engine();
    shell.machine_picker().answer_with(Some(THIS_COMPUTER));
    let grants = Arc::new(MachineGrants::default());
    let lines = Arc::new(Lines::default());
    let running = run(&engine, Some(&grants), vec![this_computer()], &lines);
    lines.wait_for("granted: []");
    lines.wait_for("picked: This computer");
    lines.wait_for(&what_this_computer_runs());
    lines.wait_for(&this_computers_login_shell());
    for line in ACTED_ON {
        lines.wait_for(line);
    }
    assert_eq!(
        shell.machine_picker().asked(),
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
    shell::wait_for_end(&running);
    for left_running in ["sleep 6011", "sleep 6012"] {
        until_gone(left_running);
    }
}

/// Waits until no process of this computer runs `command_line`, as its program's end ends it,
/// within the grace a command has to end its own way and some; fails the test otherwise.
#[cfg(unix)]
fn until_gone(command_line: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let found = std::process::Command::new("pgrep")
            .args(["-f", command_line])
            .output()
            .expect("pgrep runs");
        if found.stdout.is_empty() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{command_line} outlived the program that started it"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

// `examples/machines` runs Unix's commands, `/bin/echo` and `/bin/sh`; Windows' are tested in
// `waos-local-processes`.
#[cfg(unix)]
#[test]
fn a_machine_granted_before_needs_no_pick() {
    let (engine, shell, _arrivals) = shell::engine();
    let grants = Arc::new(MachineGrants::default());
    grants.keep_machine(THIS_COMPUTER);
    let lines = Arc::new(Lines::default());
    let running = run(&engine, Some(&grants), vec![this_computer()], &lines);
    lines.wait_for("granted: [This computer]");
    lines.wait_for("done");
    assert!(shell.machine_picker().asked().is_empty(), "nobody asked");
    shell::wait_for_end(&running);
}

#[test]
fn a_pick_cancelled_or_of_a_machine_not_offered_gives_nothing_and_keeps_nothing() {
    for answer in [None, Some("build.example.com")] {
        let (engine, shell, _arrivals) = shell::engine();
        shell.machine_picker().answer_with(answer);
        let grants = Arc::new(MachineGrants::default());
        let lines = Arc::new(Lines::default());
        let running = run(&engine, Some(&grants), vec![this_computer()], &lines);
        shell::wait_for_end(&running);
        assert_eq!(lines.all(), ["granted: []", "picked: none"], "{answer:?}");
        assert!(grants.machines().is_empty(), "{answer:?}");
    }
}

#[test]
fn a_machine_set_as_the_pickers_answer_is_picked_and_kept_unasked_and_one_not_offered_is_none() {
    for (set, kept) in [
        (THIS_COMPUTER, vec![THIS_COMPUTER]),
        ("build.example.com", Vec::new()),
    ] {
        let (engine, shell, _arrivals) = shell::engine();
        shell.machine_picker().answer_with(Some(THIS_COMPUTER));
        let grants = Arc::new(MachineGrants::answering_the_picker_with(set));
        let lines = Arc::new(Lines::default());
        let running = run(&engine, Some(&grants), vec![this_computer()], &lines);
        let picked = if kept.is_empty() {
            "picked: none"
        } else {
            "picked: This computer"
        };
        lines.wait_for(picked);
        running.stop();
        shell::wait_for_end(&running);
        assert!(
            shell.machine_picker().asked().is_empty(),
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
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.machine_picker = None;
    });
    let lines = Arc::new(Lines::default());
    let running = run(&engine, None, vec![this_computer()], &lines);
    shell::wait_for_end(&running);
    assert_eq!(lines.all(), ["granted: []", "picked: none"]);
}

#[test]
fn a_program_given_no_machines_has_none() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.machine_picker().answer_with(Some(THIS_COMPUTER));
    let lines = Arc::new(Lines::default());
    let grants = Arc::new(MachineGrants::default());
    let running = run(&engine, Some(&grants), Vec::new(), &lines);
    shell::wait_for_end(&running);
    assert_eq!(lines.all(), ["machines: none"]);
    assert!(shell.machine_picker().asked().is_empty());
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
/// `lines`.
fn run(
    engine: &WaOS,
    grants: Option<&Arc<MachineGrants>>,
    machines: Vec<Arc<dyn Machines>>,
    lines: &Arc<Lines>,
) -> RunningProgram {
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(lines) as Arc<dyn ProgramOutput>),
        grants: grants.map(|grants| Arc::clone(grants) as Arc<dyn ProgramGrants>),
        machines,
        ..ProgramEnvironment::default()
    };
    let program = shell::load(engine, waos_test_programs::MACHINES);
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    running
}

/// Grants that give nothing but the machines picked, kept as an app keeps them, and the
/// picker's answer where a test set one.
#[derive(Default)]
struct MachineGrants {
    machines: Mutex<Vec<String>>,
    answer: Option<String>,
}

impl MachineGrants {
    /// Grants whose picker of machines answers `id`, unasked.
    fn answering_the_picker_with(id: &str) -> MachineGrants {
        MachineGrants {
            answer: Some(id.to_owned()),
            ..MachineGrants::default()
        }
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

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

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
        self.answer.clone()
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
