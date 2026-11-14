//! The command a session runs: the user's own shell on the machine, as a login shell, told what
//! kind of terminal it is in.
//!
//! A machine does not say which shell its user has, nor which system it runs, so the command is
//! `/bin/sh`, which every Unix has, asked to become the shell the login environment names in
//! `SHELL`. A machine with no `/bin/sh`, as a Windows one, refuses it.

use wasdk::machines::Command;

/// What `/bin/sh` runs: the shell `SHELL` names in its place, as a login shell, or itself where
/// the variable is not set.
const BECOME_THE_USERS_SHELL: &str = "exec \"${SHELL:-/bin/sh}\" -l";

/// The terminal the screen behaves as, which programs look up to know what it understands.
const TERMINAL_TYPE: &str = "xterm-256color";

/// The user's login shell, in a terminal of this app's kind.
pub(crate) fn login_shell() -> Command {
    let command = Command::new("/bin/sh");
    command.add_argument("-c");
    command.add_argument(BECOME_THE_USERS_SHELL);
    command.variable("TERM", TERMINAL_TYPE);
    command.variable("COLORTERM", "truecolor");
    command.variable("TERM_PROGRAM", "WaOS_Terminal");
    command
}
