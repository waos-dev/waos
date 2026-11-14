//! The environment a command runs in on this computer: the user's login environment, as an SSH
//! login gives one and as VS Code reads it on this computer (the processes proposal's decision 4),
//! so that a WaOS started from the Dock still finds Homebrew's and rustup's tools. On Unix's
//! systems the user's login shell runs once and prints it (`login_shell`); on Windows, which has
//! no login shell, it is the environment Windows builds for the user's new logon
//! (`windows::user_environment`). What is read is read once and kept; a shell that fails, or takes
//! longer than the timeout, or a Windows that builds none, leaves this process's own environment in
//! its place.
//!
//! The invariant: the environment is read at most once, at the first command, and a shell never
//! outlives its timeout: one that has not answered by then is killed.

use std::ffi::OsString;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::sync::OnceCell;

/// The variables of an environment, by name.
pub type Variables = Vec<(OsString, OsString)>;

/// What measures a timeout: a future that resolves once the duration has passed.
pub type Timer = Arc<dyn Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync>;

/// How long the login shell may take, VS Code's own limit.
#[cfg(unix)]
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

/// A command's environment on this computer, read once and kept.
pub struct LoginEnvironment {
    reading: Reading,
    read: OnceCell<Variables>,
}

/// Where the environment is read from.
enum Reading {
    /// A login shell, given at most `timeout` as `timer` measures it.
    #[cfg(unix)]
    Shell {
        shell: PathBuf,
        timeout: Duration,
        timer: Timer,
    },
    /// The environment Windows builds for the user's new logon.
    #[cfg(windows)]
    UserLogon,
    /// Variables given whole, as a test gives them.
    Given(Variables),
}

impl LoginEnvironment {
    /// The user's login shell's, `$SHELL` or `/bin/sh`, given ten seconds by tokio's clock.
    #[cfg(unix)]
    pub fn of_login_shell() -> LoginEnvironment {
        let shell =
            std::env::var_os("SHELL").map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from);
        let timer: Timer = Arc::new(|wait| tokio::time::sleep(wait).boxed());
        LoginEnvironment::of_shell(shell, TIMEOUT, timer)
    }

    /// The user's logon's on Windows, which has no login shell: what Windows builds for a new
    /// logon of the user, as a process Explorer starts gets it.
    #[cfg(windows)]
    pub fn of_login_shell() -> LoginEnvironment {
        LoginEnvironment::reading(Reading::UserLogon)
    }

    /// `shell`'s, run as a login shell, given at most `timeout` as `timer` measures it.
    #[cfg(unix)]
    pub fn of_shell(shell: PathBuf, timeout: Duration, timer: Timer) -> LoginEnvironment {
        LoginEnvironment::reading(Reading::Shell {
            shell,
            timeout,
            timer,
        })
    }

    /// `variables`, whole, with no shell run.
    pub fn given(variables: Variables) -> LoginEnvironment {
        LoginEnvironment::reading(Reading::Given(variables))
    }

    fn reading(reading: Reading) -> LoginEnvironment {
        LoginEnvironment {
            reading,
            read: OnceCell::new(),
        }
    }

    /// The variables, read at the first call and kept.
    pub(crate) async fn variables(&self) -> &Variables {
        self.read.get_or_init(|| self.reading.read()).await
    }
}

impl Reading {
    async fn read(&self) -> Variables {
        match self {
            Reading::Given(variables) => variables.clone(),
            #[cfg(windows)]
            Reading::UserLogon => crate::windows::user_environment().unwrap_or_else(|error| {
                tracing::warn!("commands run in WaOS's own environment: {error}");
                std::env::vars_os().collect()
            }),
            #[cfg(unix)]
            Reading::Shell {
                shell,
                timeout,
                timer,
            } => crate::login_shell::read_shell(shell, *timeout, timer)
                .await
                .unwrap_or_else(|why| {
                    tracing::warn!(?shell, "commands run in WaOS's own environment: {why}");
                    std::env::vars_os().collect()
                }),
        }
    }
}
