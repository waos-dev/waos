//! This computer's commands, run in a login environment the test gives, never the user's own
//! shell: with pipes, their output read, their input written and closed, their variables on top
//! of the login environment, and their end told by code or by signal, an interrupt among them; and
//! in a terminal running
//! `/bin/sh`, which echoes what is typed, takes a new size, and ends with its code.

use std::future::Future;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::BoxStream;

use super::*;
use waos_capabilities::{ExitStatus, Signal};

/// How long a test waits for a command before it fails.
const WITHIN: Duration = Duration::from_secs(30);

#[test]
fn this_computer_is_the_one_machine_offered() {
    let machines = machines();
    assert_eq!(
        machines.offered(),
        [MachineChoice {
            id: THIS_COMPUTER.to_owned(),
            name: "This computer".to_owned(),
        }]
    );
    let machine = machines.machine(THIS_COMPUTER).unwrap();
    assert_eq!(machine.files(), MachineFiles::Local(PathBuf::from("/")));
    assert!(machines.machine("build.example.com").is_none());
}

#[tokio::test]
async fn a_command_s_output_is_read_and_it_ends_with_its_code() {
    within(async {
        let started = exec(command("/bin/echo", &["hello"])).await.unwrap();
        assert_eq!(all_of(started.output).await, "hello\n");
        assert_eq!(all_of(started.errors).await, "");
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(0));
    })
    .await;
}

#[tokio::test]
async fn a_command_reads_its_input_to_the_end_once_it_is_closed() {
    within(async {
        let started = exec(command("/bin/cat", &[])).await.unwrap();
        started.input.write(b"one ".to_vec()).await.unwrap();
        started.input.write(b"two".to_vec()).await.unwrap();
        started.input.close();
        assert_eq!(all_of(started.output).await, "one two");
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(0));
    })
    .await;
}

#[tokio::test]
async fn a_command_s_variables_go_on_top_of_the_login_environment() {
    within(async {
        let mut env = command("/usr/bin/env", &[]);
        env.variables = vec![("FROM_LOGIN".to_owned(), "replaced".to_owned())];
        let started = exec(env).await.unwrap();
        let printed = all_of(started.output).await;
        let mut lines: Vec<&str> = printed.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            ["FROM_LOGIN=replaced", "HOME=/", "PATH=/usr/bin:/bin"]
        );
    })
    .await;
}

#[tokio::test]
async fn a_program_that_is_not_there_is_refused() {
    within(async {
        let refused = exec(command("/no/such/program", &[])).await;
        assert!(matches!(refused, Err(ExecError::Refused(_))));
    })
    .await;
}

#[tokio::test]
async fn a_signal_ends_a_command_and_its_end_names_it() {
    within(async {
        let started = exec(command("/bin/sleep", &["600"])).await.unwrap();
        started.child.signal(Signal::Terminate);
        assert_eq!(
            started.child.exit_status().await,
            ExitStatus::Signaled("TERM".to_owned())
        );
        assert_eq!(
            started.child.exit_status().await,
            ExitStatus::Signaled("TERM".to_owned()),
            "every call answers the same"
        );
    })
    .await;
}

#[tokio::test]
async fn an_interrupt_ends_a_command_run_with_pipes_as_ctrl_c_would() {
    within(async {
        let started = exec(command("/bin/sleep", &["600"])).await.unwrap();
        started.child.signal(Signal::Interrupt);
        assert_eq!(
            started.child.exit_status().await,
            ExitStatus::Signaled("INT".to_owned())
        );
    })
    .await;
}

#[tokio::test]
async fn a_shell_in_a_terminal_echoes_what_is_typed_takes_a_new_size_and_ends() {
    within(async {
        let size = TerminalSize {
            columns: 80,
            rows: 24,
            ..TerminalSize::default()
        };
        let mut started = machine()
            .exec_in_terminal(command("/bin/sh", &[]), size)
            .await
            .unwrap();
        started
            .terminal
            .write(b"echo $((6 * 7))\n".to_vec())
            .await
            .unwrap();
        read_until(&mut started.output, "42").await;

        started.terminal.resize(TerminalSize {
            columns: 100,
            rows: 30,
            ..TerminalSize::default()
        });
        started
            .terminal
            .write(b"stty size\n".to_vec())
            .await
            .unwrap();
        read_until(&mut started.output, "30 100").await;

        started.terminal.write(b"exit 4\n".to_vec()).await.unwrap();
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(4));
    })
    .await;
}

#[tokio::test]
async fn a_dropped_terminal_hangs_its_command_up() {
    within(async {
        let script = "trap 'exit 7' HUP; echo ready; while :; do sleep 0.1; done";
        let mut started = machine()
            .exec_in_terminal(command("/bin/sh", &["-c", script]), TerminalSize::default())
            .await
            .unwrap();
        read_until(&mut started.output, "ready").await;
        let exit = started.child.exit_status();
        drop(started.terminal);
        assert_eq!(exit.await, ExitStatus::Exited(7));
    })
    .await;
}

#[tokio::test]
async fn an_ended_command_is_asked_to_end_its_own_way_and_killed_where_it_does_not() {
    within(async {
        let mut piped = exec(command(
            "/bin/sh",
            &[
                "-c",
                "trap 'exit 3' TERM; echo ready; while :; do sleep 0.1; done",
            ],
        ))
        .await
        .unwrap();
        read_until(&mut piped.output, "ready").await;
        let script = "trap 'exit 5' HUP; echo ready; while :; do sleep 0.1; done";
        let mut in_terminal = machine()
            .exec_in_terminal(command("/bin/sh", &["-c", script]), TerminalSize::default())
            .await
            .unwrap();
        read_until(&mut in_terminal.output, "ready").await;
        let mut stubborn = exec(command(
            "/bin/sh",
            &[
                "-c",
                "trap '' TERM; echo ready; while :; do sleep 0.1; done",
            ],
        ))
        .await
        .unwrap();
        // Each shell says so once its trap is set.
        read_until(&mut stubborn.output, "ready").await;

        piped.child.end();
        in_terminal.child.end();
        stubborn.child.end();
        assert_eq!(piped.child.exit_status().await, ExitStatus::Exited(3));
        assert_eq!(in_terminal.child.exit_status().await, ExitStatus::Exited(5));
        assert_eq!(
            stubborn.child.exit_status().await,
            ExitStatus::Signaled("KILL".to_owned()),
            "killed once its grace is over"
        );
    })
    .await;
}

/// A command started with pipes on a runtime that then goes, as a program's goes as it ends: its
/// exit status still answers how it really ended, so the end WaOS gives it is heard.
#[test]
fn a_command_s_end_is_heard_after_the_runtime_that_started_it_has_gone() {
    let program = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let started = program.block_on(exec(command("/bin/sh", &["-c", "sleep 1; exit 7"])));
    let exit = started.unwrap().child.exit_status();
    drop(program);
    assert_eq!(futures::executor::block_on(exit), ExitStatus::Exited(7));
}

/// This computer's provider, its commands run with `PATH` and `HOME` alone.
fn machines() -> LocalMachines {
    LocalMachines::new(LoginEnvironment::given(vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), "/".into()),
        ("FROM_LOGIN".into(), "kept".into()),
    ]))
}

fn machine() -> Arc<dyn Machine> {
    machines().machine(THIS_COMPUTER).unwrap()
}

async fn exec(command: Command) -> Result<WithPipes, ExecError> {
    machine().exec(command).await
}

fn command(program: &str, arguments: &[&str]) -> Command {
    Command {
        program: program.to_owned(),
        arguments: arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
        ..Command::default()
    }
}

/// Everything `output` gives until it ends, as text.
async fn all_of(output: BoxStream<'static, Vec<u8>>) -> String {
    let bytes: Vec<u8> = output.concat().await;
    String::from_utf8(bytes).unwrap()
}

/// Reads `output` until what it gave holds `text`.
async fn read_until(output: &mut BoxStream<'static, Vec<u8>>, text: &str) {
    let mut read = String::new();
    while !read.contains(text) {
        let chunk = output
            .next()
            .await
            .unwrap_or_else(|| panic!("the output ended before {text:?}: {read:?}"));
        read.push_str(&String::from_utf8_lossy(&chunk));
    }
}

/// `test`, failing after [`WITHIN`].
async fn within(test: impl Future<Output = ()>) {
    tokio::time::timeout(WITHIN, test)
        .await
        .expect("the commands answered in time");
}
