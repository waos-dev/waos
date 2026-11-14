//! The user's login shell, run once to print its environment, on Unix's systems: interactive and a
//! login shell, in a terminal of its own, as a terminal app starts it, so a shell that takes its
//! terminal finds one and never stops WaOS's; its environment printed between two marks, which
//! keep apart what its startup files print.
//!
//! The invariant: a shell that has not answered within its timeout is killed.

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::ffi::OsStringExt as _;
use std::path::Path;
use std::thread;
use std::time::Duration;

use futures::channel::oneshot;
use futures::future::{Either, select};
use portable_pty::{ChildKiller, CommandBuilder, PtySize, native_pty_system};

use crate::login_environment::{Timer, Variables};

/// What the shell prints before its environment and after it.
const MARK: &str = "__WAOS_LOGIN_ENVIRONMENT__";

/// What `shell` prints of its environment, run as an interactive login shell in a terminal of
/// its own; or why it printed none within `timeout`.
pub(crate) async fn read_shell(
    shell: &Path,
    timeout: Duration,
    timer: &Timer,
) -> Result<Variables, String> {
    let ShellRun {
        printed,
        mut killer,
    } = run_in_a_terminal(shell)?;
    match select(printed, timer(timeout)).await {
        Either::Left((printed, _)) => {
            let printed = printed.map_err(|_| "the shell's reader ended".to_owned())?;
            variables_between_marks(&printed)
                .ok_or_else(|| "the shell printed no environment".to_owned())
        }
        Either::Right(((), _)) => {
            // A shell that has not answered is ended, which ends its terminal and its reader.
            let _ = killer.kill();
            Err(format!("the shell took longer than {timeout:?}"))
        }
    }
}

/// A login shell started: what it prints, once its terminal closes, and what ends it.
struct ShellRun {
    printed: oneshot::Receiver<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

/// Starts `shell` printing its environment in a terminal of its own, and reads what it prints on
/// a thread until the terminal closes.
fn run_in_a_terminal(shell: &Path) -> Result<ShellRun, String> {
    let failed = |error: anyhow::Error| format!("{error:#}");
    let terminal = native_pty_system()
        .openpty(PtySize::default())
        .map_err(failed)?;
    let mut reader = terminal.master.try_clone_reader().map_err(failed)?;
    let mut command = CommandBuilder::new(shell);
    command.args(["-i", "-l", "-c", &print_environment()]);
    let mut child = terminal.slave.spawn_command(command).map_err(failed)?;
    // Only the shell holds the terminal's other end, so the terminal closes when it ends.
    drop(terminal.slave);
    let killer = child.clone_killer();
    let (printed, read) = oneshot::channel();
    thread::Builder::new()
        .name("login shell".to_owned())
        .spawn(move || {
            let mut bytes = Vec::new();
            // Linux answers a terminal whose other end has closed with an error, not an end.
            let _ = reader.read_to_end(&mut bytes);
            let _ = child.wait();
            drop(terminal.master);
            let _ = printed.send(bytes);
        })
        .map_err(|error| error.to_string())?;
    Ok(ShellRun {
        printed: read,
        killer,
    })
}

/// What the shell runs: its environment, between two marks.
fn print_environment() -> String {
    format!("printf '\\n%s\\n' {MARK}; env; printf '\\n%s\\n' {MARK}")
}

/// The variables `printed` holds between the two marks, each `NAME=value` on a line of its own, a
/// line without one continuing the value before; none without both marks.
fn variables_between_marks(printed: &[u8]) -> Option<Variables> {
    let text: Vec<&[u8]> = printed
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .collect();
    let mark = MARK.as_bytes();
    let first = text.iter().position(|line| *line == mark)?;
    let last = first + 1 + text[first + 1..].iter().position(|line| *line == mark)?;
    let mut variables: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for line in &text[first + 1..last] {
        match (split_variable(line), variables.last_mut()) {
            (Some((name, value)), _) => variables.push((name.to_vec(), value.to_vec())),
            (None, Some((_, value))) => {
                value.push(b'\n');
                value.extend_from_slice(line);
            }
            (None, None) => {}
        }
    }
    Some(
        variables
            .into_iter()
            .map(|(name, value)| (OsString::from_vec(name), OsString::from_vec(value)))
            .collect(),
    )
}

/// `line`'s name and value, where it starts a variable: a name of letters, digits and
/// underscores, not starting with a digit, then `=`.
fn split_variable(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let equals = line.iter().position(|byte| *byte == b'=')?;
    let name = &line[..equals];
    let is_name = !name.is_empty()
        && !name[0].is_ascii_digit()
        && name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
    is_name.then(|| (name, &line[equals + 1..]))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    use std::sync::Arc;

    use futures::FutureExt as _;
    use futures::future::{pending, ready};

    use super::*;
    use crate::login_environment::{LoginEnvironment, TIMEOUT};

    #[test]
    fn the_environment_is_what_lies_between_the_marks_a_value_running_on_over_lines() {
        let printed = format!(
            "a greeting from a startup file\r\n\r\n{MARK}\r\nA=1\r\nB=two\r\nlines\r\n\r\n{MARK}\r\n"
        );
        let variables = variables_between_marks(printed.as_bytes()).unwrap();
        assert_eq!(variables, [pair("A", "1"), pair("B", "two\nlines\n")]);
        assert_eq!(variables_between_marks(b"no marks"), None);
    }

    #[tokio::test]
    async fn a_login_shell_is_read_once_in_a_terminal() {
        let scratch = tempfile::tempdir().unwrap();
        let count = scratch.path().join("runs");
        let shell = shell_script(
            scratch.path(),
            &format!(
                "echo run >> {}\nprintf 'motd\\n{MARK}\\nPATH=/opt/tools/bin\\nTERMINAL=%s\\n{MARK}\\n' \"$(test -t 0 && echo yes)\"",
                count.display()
            ),
        );
        let environment = LoginEnvironment::of_shell(shell, TIMEOUT, never());
        let expected = [pair("PATH", "/opt/tools/bin"), pair("TERMINAL", "yes")];
        assert_eq!(environment.variables().await, &expected);
        assert_eq!(environment.variables().await, &expected);
        assert_eq!(fs::read_to_string(count).unwrap(), "run\n", "read once");
    }

    #[tokio::test]
    async fn a_shell_that_fails_or_takes_too_long_leaves_waos_s_own_environment() {
        let scratch = tempfile::tempdir().unwrap();
        let own: Variables = std::env::vars_os().collect();
        let missing = LoginEnvironment::of_shell(scratch.path().join("no-shell"), TIMEOUT, never());
        assert_eq!(missing.variables().await, &own);

        let silent = shell_script(scratch.path(), "exit 3");
        let silent = LoginEnvironment::of_shell(silent, TIMEOUT, never());
        assert_eq!(silent.variables().await, &own);

        let hanging = shell_script(scratch.path(), "exec sleep 600");
        let timed_out: Timer = Arc::new(|_| ready(()).boxed());
        let hanging = LoginEnvironment::of_shell(hanging, TIMEOUT, timed_out);
        assert_eq!(hanging.variables().await, &own);
    }

    fn pair(name: &str, value: &str) -> (OsString, OsString) {
        (name.into(), value.into())
    }

    /// A timer that never fires.
    fn never() -> Timer {
        Arc::new(|_| pending().boxed())
    }

    /// A shell, at a path in `folder` of its own, that runs `script` whatever it is asked.
    fn shell_script(folder: &Path, script: &str) -> PathBuf {
        let path = folder.join(format!("shell-{}", fs::read_dir(folder).unwrap().count()));
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}
