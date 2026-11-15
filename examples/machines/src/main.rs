//! A program that acts on a machine its user picked for it, in order: it says which machines it
//! holds, asks its user to pick one when it holds none, and says which it holds after; says what
//! the machine runs and its user's login shell; then, on the machine, it runs `/bin/echo` with pipes and says what it printed and how it ended; makes a
//! file in a new folder with a command, with a symlink to it holding its absolute path, and reads it
//! back from the machine's root by its absolute path, through the symlink, and from above `/`; runs `/bin/sh` in a terminal, types a sum and reads the answer, resizes the terminal and
//! reads its new size with `stty`, and types `exit 4`; starts `/bin/sleep` twice, ends one
//! with `terminate` and the other with `interrupt`, and says how each ended; and starts it once
//! more with pipes and once in a terminal, and ends while both run, which ends them.
//!
//! It prints each step as `what: result`, and `done` at the end.
//!
//! ```text
//! cargo build -p machines --target wasm32-wasip3
//! ```

use wasdk::StreamReader;
use wasdk::filesystem::{DescriptorFlags, OpenFlags, PathFlags};
use wasdk::machines::{
    self, Architecture, Command, ExitStatus, Machine, OperatingSystem, Signal, TerminalSize,
};

/// What makes the file the program reads back, with a symlink to it holding its absolute path,
/// and prints the folder they are in.
const MAKE_A_FILE: &str = "folder=$(mktemp -d) && printf 'from the machine' > \"$folder/note.txt\" && ln -s \"$folder/note.txt\" \"$folder/link\" && printf %s \"$folder\"";

#[wasdk::main]
async fn main() {
    let Some(machines) = machines::get_machines() else {
        println!("machines: none");
        return;
    };
    println!("granted: {}", names(&machines.granted()));
    let machine = match machines.granted().into_iter().next() {
        Some(machine) => machine,
        None => match machines.pick().await {
            Some(machine) => {
                println!("picked: {}", machine.name());
                machine
            }
            None => {
                println!("picked: none");
                return;
            }
        },
    };
    println!("granted after: {}", names(&machines.granted()));
    say_what_it_runs(&machine);
    echo(&machine).await;
    read_from_the_root(&machine).await;
    type_in_a_terminal(&machine).await;
    end_with_a_signal(&machine).await;
    let _running = leave_running(&machine).await;
    println!("done");
}

/// Says what the machine runs, as `uname -sm` names it, and its user's login shell.
fn say_what_it_runs(machine: &Machine) {
    let platform = machine.platform();
    let os = match platform.os {
        OperatingSystem::Macos => "macos".to_owned(),
        OperatingSystem::Linux => "linux".to_owned(),
        OperatingSystem::Windows => "windows".to_owned(),
        OperatingSystem::Other(name) => name,
    };
    let arch = match platform.arch {
        Architecture::Aarch64 => "aarch64".to_owned(),
        Architecture::X8664 => "x86-64".to_owned(),
        Architecture::Other(name) => name,
    };
    println!("platform: {os} {arch}");
    let shell = machine.login_shell();
    println!("login shell: {}", shell.as_deref().unwrap_or("none"));
}

/// Runs `/bin/echo hello` with pipes, and says what it printed and how it ended.
async fn echo(machine: &Machine) {
    let echo = command("/bin/echo", &["hello"]);
    let (child, pipes) = machine.exec(&echo).await.expect("echo runs");
    let printed = pipes.output().expect("its output").collect().await;
    println!("echo: {}", String::from_utf8_lossy(&printed).trim_end());
    println!("echo ended: {}", ended(child.exit_status().await));
}

/// Makes a file in a new folder with a command, with a symlink to it holding its absolute path;
/// reads it from the machine's root by its absolute path, through the symlink, and from above
/// `/`; and removes the folder.
async fn read_from_the_root(machine: &Machine) {
    let folder = output_of(machine, command("/bin/sh", &["-c", MAKE_A_FILE])).await;
    let from_root = folder.trim_start_matches('/');
    let root = machine.root();
    for (how, path) in [
        ("root read", format!("{from_root}/note.txt")),
        (
            "root read through an absolute symlink",
            format!("{from_root}/link"),
        ),
        (
            "root read from above /",
            format!("../../{from_root}/note.txt"),
        ),
    ] {
        let file = root
            .open_at(
                PathFlags::SYMLINK_FOLLOW,
                path,
                OpenFlags::empty(),
                DescriptorFlags::READ,
            )
            .await
            .expect("the file opens from the root");
        let (contents, ended) = file.read_via_stream(0);
        let bytes = contents.collect().await;
        ended.await.expect("the file reads");
        println!("{how}: {}", String::from_utf8_lossy(&bytes));
    }
    output_of(machine, command("/bin/rm", &["-r", &folder])).await;
}

/// Runs `/bin/sh` in a terminal: types a sum and reads the answer, resizes the terminal and reads
/// its size, and types `exit 4`.
async fn type_in_a_terminal(machine: &Machine) {
    let size = TerminalSize {
        columns: 80,
        rows: 24,
        width_in_pixels: 0,
        height_in_pixels: 0,
    };
    let (child, terminal) = machine
        .exec_in_terminal(&command("/bin/sh", &[]), size)
        .await
        .expect("a shell runs in a terminal");
    let mut output = terminal.output().expect("its output");
    terminal
        .write(b"echo $((6 * 7))\n".to_vec())
        .await
        .expect("typed");
    read_until(&mut output, "42").await;
    println!("terminal: 42");
    terminal.resize(TerminalSize {
        columns: 100,
        rows: 30,
        ..size
    });
    terminal
        .write(b"stty size\n".to_vec())
        .await
        .expect("typed");
    read_until(&mut output, "30 100").await;
    println!("terminal resized: 30 100");
    terminal.write(b"exit 4\n".to_vec()).await.expect("typed");
    drop(output);
    println!("terminal ended: {}", ended(child.exit_status().await));
}

/// Starts `/bin/sleep` twice, ends one with `terminate` and the other with `interrupt`, as Ctrl-C
/// would, and says how each ended.
async fn end_with_a_signal(machine: &Machine) {
    for (how, signal) in [
        ("signaled", Signal::Terminate),
        ("interrupted", Signal::Interrupt),
    ] {
        let (child, _pipes) = machine
            .exec(&command("/bin/sleep", &["600"]))
            .await
            .expect("sleep runs");
        child.signal(signal);
        println!("{how}: {}", ended(child.exit_status().await));
    }
}

/// Starts `/bin/sleep 6011` with pipes and `/bin/sleep 6012` in a terminal, and answers them,
/// running, with the terminal, for the program to hold as it ends.
async fn leave_running(
    machine: &Machine,
) -> (machines::Child, machines::Child, machines::Terminal) {
    let (piped, _pipes) = machine
        .exec(&command("/bin/sleep", &["6011"]))
        .await
        .expect("sleep runs");
    let size = TerminalSize {
        columns: 80,
        rows: 24,
        width_in_pixels: 0,
        height_in_pixels: 0,
    };
    let (in_terminal, terminal) = machine
        .exec_in_terminal(&command("/bin/sleep", &["6012"]), size)
        .await
        .expect("sleep runs in a terminal");
    println!("left running: sleep 6011, sleep 6012");
    (piped, in_terminal, terminal)
}

/// What `command` prints, once it has ended.
async fn output_of(machine: &Machine, command: Command) -> String {
    let (child, pipes) = machine.exec(&command).await.expect("the command runs");
    let printed = pipes.output().expect("its output").collect().await;
    child.exit_status().await;
    String::from_utf8_lossy(&printed).into_owned()
}

/// Reads `output` until what it gave holds `text`.
async fn read_until(output: &mut StreamReader<u8>, text: &str) {
    let mut read = Vec::new();
    while !String::from_utf8_lossy(&read).contains(text) {
        let (_result, chunk) = output.read(Vec::with_capacity(1024)).await;
        assert!(!chunk.is_empty(), "the terminal closed before {text:?}");
        read.extend(chunk);
    }
}

fn command(program: &str, arguments: &[&str]) -> Command {
    let command = Command::new(program);
    for argument in arguments {
        command.add_argument(argument);
    }
    command
}

fn names(machines: &[Machine]) -> String {
    let names: Vec<String> = machines.iter().map(Machine::name).collect();
    format!("[{}]", names.join(", "))
}

fn ended(status: ExitStatus) -> String {
    match status {
        ExitStatus::Exited(code) => format!("exited {code}"),
        ExitStatus::Signaled(signal) => format!("signaled {signal}"),
        ExitStatus::Unknown => "unknown".to_owned(),
    }
}
