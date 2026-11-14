//! This computer's commands on Windows, run in a login environment the test gives, with the
//! system's `cmd.exe`, `findstr` and `ping`: with pipes, their output read, their input written and
//! closed, their variables on top of the login environment, and their end told by code, every
//! process they started ended with their job; and in a pseudoconsole running `cmd.exe`, which runs
//! what is typed, takes a new size and ends with its code, is interrupted by Ctrl-C, which ends a
//! command by the signal `INT`, and closes with its terminal.

use std::future::Future;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::BoxStream;

use super::*;
use waos_capabilities::{ExitStatus, Signal};

/// How long a test waits for a command before it fails.
const WITHIN: Duration = Duration::from_secs(30);

/// Where Windows' own programs are.
const SYSTEM32: &str = r"C:\Windows\System32";

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
    assert!(machines.machine("build.example.com").is_none());
}

#[tokio::test]
async fn a_command_s_output_is_read_and_it_ends_with_its_code() {
    within(async {
        let started = exec(command("cmd.exe", &["/c", "echo hello& exit 3"]))
            .await
            .unwrap();
        assert_eq!(all_of(started.output).await, "hello\r\n");
        assert_eq!(all_of(started.errors).await, "");
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(3));
    })
    .await;
}

#[tokio::test]
async fn a_command_reads_its_input_to_the_end_once_it_is_closed() {
    within(async {
        let started = exec(command("findstr.exe", &["^"])).await.unwrap();
        started.input.write(b"one\r\n".to_vec()).await.unwrap();
        started.input.write(b"two\r\n".to_vec()).await.unwrap();
        started.input.close();
        assert_eq!(all_of(started.output).await, "one\r\ntwo\r\n");
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(0));
    })
    .await;
}

#[tokio::test]
async fn a_command_s_variables_go_on_top_of_the_login_environment() {
    within(async {
        let mut echo = command("cmd.exe", &["/c", "echo %FROM_LOGIN% %ADDED%"]);
        echo.variables = vec![
            ("FROM_LOGIN".to_owned(), "replaced".to_owned()),
            ("ADDED".to_owned(), "added".to_owned()),
        ];
        let started = exec(echo).await.unwrap();
        assert_eq!(all_of(started.output).await, "replaced added\r\n");
    })
    .await;
}

#[tokio::test]
async fn a_command_starts_in_the_user_s_home_unless_it_names_a_folder() {
    within(async {
        let started = exec(command("cmd.exe", &["/c", "cd"])).await.unwrap();
        assert_eq!(all_of(started.output).await, format!("{SYSTEM32}\r\n"));
    })
    .await;
}

#[tokio::test]
async fn a_program_that_is_not_there_is_refused() {
    within(async {
        let refused = exec(command(r"C:\no\such\program.exe", &[])).await;
        assert!(matches!(refused, Err(ExecError::Refused(_))));
    })
    .await;
}

#[tokio::test]
async fn a_signal_ends_every_process_of_the_command_s_job() {
    within(async {
        // `cmd` starts `ping`, which holds the output open: the output ends only once both have.
        let started = exec(command("cmd.exe", &["/c", "ping -n 600 127.0.0.1"]))
            .await
            .unwrap();
        let mut output = started.output;
        read_until(&mut output, "127.0.0.1").await;
        started.child.signal(Signal::Terminate);
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(1));
        while output.next().await.is_some() {}
    })
    .await;
}

#[tokio::test]
async fn an_interrupt_ends_a_command_run_with_pipes_with_its_job() {
    within(async {
        let started = exec(command("ping.exe", &["-n", "600", "127.0.0.1"]))
            .await
            .unwrap();
        started.child.signal(Signal::Interrupt);
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(1));
    })
    .await;
}

#[tokio::test]
async fn a_shell_in_a_terminal_runs_what_is_typed_takes_a_new_size_and_ends() {
    within(async {
        let mut started = machine()
            .exec_in_terminal(command("cmd.exe", &[]), size(80, 24))
            .await
            .unwrap();
        started
            .terminal
            .write(b"set /a 6*7\r".to_vec())
            .await
            .unwrap();
        read_until(&mut started.output, "42").await;

        started.terminal.resize(size(100, 30));
        started
            .terminal
            .write(b"mode con\r".to_vec())
            .await
            .unwrap();
        read_until(&mut started.output, "100").await;

        started.terminal.write(b"exit 4\r".to_vec()).await.unwrap();
        assert_eq!(started.child.exit_status().await, ExitStatus::Exited(4));
        while started.output.next().await.is_some() {}
    })
    .await;
}

#[tokio::test]
async fn an_interrupt_is_ctrl_c_in_the_terminal_and_ends_a_command_by_int() {
    within(async {
        let mut started = machine()
            .exec_in_terminal(command("ping.exe", &["-t", "127.0.0.1"]), size(80, 24))
            .await
            .unwrap();
        read_until(&mut started.output, "127.0.0.1").await;
        started.child.signal(Signal::Interrupt);
        assert_eq!(
            started.child.exit_status().await,
            ExitStatus::Signaled("INT".to_owned())
        );
    })
    .await;
}

#[tokio::test]
async fn a_dropped_terminal_closes_its_command_s_console() {
    within(async {
        let mut started = machine()
            .exec_in_terminal(command("cmd.exe", &[]), size(80, 24))
            .await
            .unwrap();
        started
            .terminal
            .write(b"echo ready\r".to_vec())
            .await
            .unwrap();
        read_until(&mut started.output, "ready").await;
        let exit = started.child.exit_status();
        drop(started.terminal);
        assert!(matches!(
            exit.await,
            ExitStatus::Exited(_) | ExitStatus::Signaled(_)
        ));
    })
    .await;
}

/// This computer's provider, its commands run with Windows' own programs on `PATH`, at home in
/// them.
fn machines() -> LocalMachines {
    LocalMachines::new(LoginEnvironment::given(vec![
        ("PATH".into(), SYSTEM32.into()),
        ("PATHEXT".into(), ".COM;.EXE".into()),
        ("SystemRoot".into(), r"C:\Windows".into()),
        ("USERPROFILE".into(), SYSTEM32.into()),
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

fn size(columns: u16, rows: u16) -> TerminalSize {
    TerminalSize {
        columns,
        rows,
        ..TerminalSize::default()
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
