//! How a command starts on this computer: its program and arguments, its variables, the
//! login environment's with the command's on top (the processes proposal's decision 4), and its
//! folder, the one it names or the user's home: `HOME`, or `USERPROFILE` on Windows.

use std::ffi::OsString;
use std::path::PathBuf;

use waos_capabilities::Command;

use crate::login_environment::Variables;

/// A command as it starts.
pub(crate) struct Starting {
    pub(crate) program: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) variables: Variables,
    pub(crate) folder: PathBuf,
}

/// `command` as it starts in `login`, the login environment.
pub(crate) fn starting(command: &Command, login: &Variables) -> Starting {
    let mut variables = login.clone();
    for (name, value) in &command.variables {
        let name = OsString::from(name);
        variables.retain(|(kept, _)| *kept != name);
        variables.push((name, OsString::from(value)));
    }
    let folder = command
        .working_directory
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| home_in(&variables))
        .unwrap_or_else(|| PathBuf::from("/"));
    Starting {
        program: command.program.clone(),
        arguments: command.arguments.clone(),
        variables,
        folder,
    }
}

/// The variable that names the user's home.
const HOME: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

/// `variables` with `home` as the user's home, in place of the one they named.
pub(crate) fn set_home(variables: &mut Variables, home: PathBuf) {
    variables.retain(|(name, _)| name != HOME);
    variables.push((HOME.into(), home.into_os_string()));
}

/// The user's home folder, as `variables` name it.
fn home_in(variables: &Variables) -> Option<PathBuf> {
    variables
        .iter()
        .find(|(name, _)| name == HOME)
        .map(|(_, home)| PathBuf::from(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_s_variables_go_on_top_of_the_login_environment_and_it_starts_at_home() {
        let login: Variables = vec![
            (HOME.into(), "/Users/someone".into()),
            ("PATH".into(), "/opt/homebrew/bin:/usr/bin".into()),
        ];
        let mut command = Command::new("make");
        command.variables = vec![
            ("PATH".to_owned(), "/tools".to_owned()),
            ("CI".to_owned(), "1".to_owned()),
        ];
        let started = starting(&command, &login);
        assert_eq!(
            started.variables,
            [
                (HOME.into(), "/Users/someone".into()),
                ("PATH".into(), "/tools".into()),
                ("CI".into(), "1".into()),
            ]
        );
        assert_eq!(started.folder, PathBuf::from("/Users/someone"));

        command.working_directory = Some("/tmp".to_owned());
        assert_eq!(starting(&command, &login).folder, PathBuf::from("/tmp"));
    }

    #[test]
    fn a_home_given_takes_the_place_of_the_login_environment_s() {
        let mut login: Variables = vec![
            (HOME.into(), "/".into()),
            ("PATH".into(), "/system/bin".into()),
        ];
        set_home(&mut login, PathBuf::from("/storage/emulated/0"));
        assert_eq!(
            login,
            [
                ("PATH".into(), "/system/bin".into()),
                (HOME.into(), "/storage/emulated/0".into()),
            ]
        );
        let started = starting(&Command::new("ls"), &login);
        assert_eq!(started.folder, PathBuf::from("/storage/emulated/0"));
    }
}
