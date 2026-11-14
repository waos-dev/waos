//! Starting the WaOS app installed on this machine, for a run when no WaOS answers, with its log
//! kept where the user's logs are: on macOS WaOS.app, which Launch Services finds; on Linux the
//! app its desktop entry starts (`desktop_entries`), as the desktop's launcher starts it; on
//! Windows the app its installer recorded among the installed apps.
//!
//! The invariant: the app is found as the system finds an installed app, by its identifier or the
//! name its installer recorded, never by a path in a checkout or on the `PATH`: whoever writes a
//! program has WaOS installed, as Expo Go is installed for a project's author.

use std::path::PathBuf;

use anyhow::Result;

/// The app's identifier, as `apps/waos`'s `[package.metadata.waft]` sets it.
#[cfg(not(windows))]
pub const BUNDLE_IDENTIFIER: &str = "dev.waos.app";

/// What WaOS is started with: its launcher closed, so a development run shows only the program;
/// the launcher shows at a click on WaOS's icon in the Dock or the taskbar.
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
const LAUNCHER_CLOSED: &str = "--launcher-closed";

/// What WaOS reads from its environment, passed on when set here: `RUST_LOG`, what its log
/// holds, and `WAOS_NATIVE_UNWIND_INFO`, whether it compiles programs with what a native profiler
/// or debugger needs to walk through their frames.
#[cfg(target_os = "macos")]
const PASSED_ON: [&str; 2] = ["RUST_LOG", "WAOS_NATIVE_UNWIND_INFO"];

/// Starts WaOS.app, as the Finder would but with its launcher closed ([`LAUNCHER_CLOSED`]),
/// and answers where its log goes once the system has taken the request; the app answers on its
/// socket a moment later.
///
/// What WaOS writes to standard error, its log, is appended to `~/Library/Logs/WaOS.log`, and
/// the variables of [`PASSED_ON`] set here are passed on. All three take effect only when the
/// system starts the app: a WaOS running already keeps its own.
#[cfg(target_os = "macos")]
pub fn start() -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::process::Command;

    use anyhow::{Context, bail};

    let log = log_path();
    let mut open = Command::new("open");
    open.args(["-b", BUNDLE_IDENTIFIER])
        .arg("--stderr")
        .arg(&log);
    for name in PASSED_ON {
        if let Some(value) = std::env::var_os(name) {
            let mut variable = OsString::from(format!("{name}="));
            variable.push(value);
            open.arg("--env").arg(variable);
        }
    }
    // What follows `--args` is WaOS's own command line, and comes last.
    open.args(["--args", LAUNCHER_CLOSED]);
    let status = open.status().context("running open")?;
    if !status.success() {
        bail!(
            "WaOS.app is not installed: build it with `cargo waft build macos -p waos-app` and open it once, so the system knows where it is"
        );
    }
    Ok(log)
}

/// Where a WaOS started here keeps its log: the user's logs, where Console finds it.
#[cfg(target_os = "macos")]
fn log_path() -> PathBuf {
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join("Library/Logs/WaOS.log")
}

/// Starts the WaOS the desktop entry `dev.waos.app.desktop` starts, as the desktop's launcher
/// would but with its launcher closed ([`LAUNCHER_CLOSED`]), in a process group of its own so
/// that Ctrl-C here does not reach it, and answers where its log goes; the app answers on its
/// socket a moment later.
///
/// What WaOS writes to standard output and standard error, its log, is appended to
/// `$XDG_STATE_HOME/waos/waos.log`, or `~/.local/state/waos/waos.log`. WaOS gets this process's
/// environment, `RUST_LOG` and `WAOS_NATIVE_UNWIND_INFO` among it.
#[cfg(target_os = "linux")]
pub fn start() -> Result<PathBuf> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    use anyhow::Context;

    use crate::desktop_entries;

    let entry = desktop_entries::find(BUNDLE_IDENTIFIER).with_context(|| {
        format!(
            "WaOS is not installed: no {BUNDLE_IDENTIFIER}.desktop among the desktop's applications; build its package with `cargo waft build deb -p waos-app` and install it"
        )
    })?;
    let command = desktop_entries::command(&entry)?;
    let log = log_path()?;
    if let Some(folder) = log.parent() {
        fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    }
    let output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .with_context(|| format!("opening {}", log.display()))?;
    Command::new(&command[0])
        .args(&command[1..])
        .arg(LAUNCHER_CLOSED)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {}, as {} says", command[0], entry.display()))?;
    Ok(log)
}

/// Where a WaOS started here keeps its log: the user's state folder, where the XDG base
/// directories keep logs.
#[cfg(target_os = "linux")]
fn log_path() -> Result<PathBuf> {
    use std::path::Path;

    use anyhow::Context;

    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(folder) if Path::new(&folder).is_absolute() => PathBuf::from(folder),
        _ => std::env::home_dir()
            .context("this system names no home folder to keep WaOS's log in")?
            .join(".local/state"),
    };
    Ok(state.join("waos/waos.log"))
}

/// The app's name among the installed apps, as `apps/waos`'s `[package.metadata.waft]` names it,
/// which its installer records it under.
#[cfg(windows)]
const INSTALLED_NAME: &str = "WaOS";

/// The app's executable in its install folder, as `apps/waos`'s `[[bin]]` names it.
#[cfg(windows)]
const EXECUTABLE: &str = "waos.exe";

/// Starts the installed WaOS with its launcher closed ([`LAUNCHER_CLOSED`]), on its own,
/// apart from this terminal, and answers where its log goes; the app answers on its pipe a moment
/// later.
///
/// WaOS is found where its installer says it installed it, the folder its entry among the
/// installed apps records (`InstallLocation`). What it writes to standard error, its log, is
/// appended to `%LOCALAPPDATA%\WaOS\Logs\WaOS.log`, and its standard output goes nowhere; with
/// both redirected it does not attach to this terminal's console, so a Ctrl-C here, which ends the
/// run, does not end WaOS. Not in a process group of its own, which would start WaOS ignoring
/// Ctrl-C and pass that on to the commands its programs run in a terminal, whose interrupt is a
/// Ctrl-C. It inherits this process's environment, `RUST_LOG` and
/// `WAOS_NATIVE_UNWIND_INFO` among it; a WaOS running already keeps its own.
#[cfg(windows)]
pub fn start() -> Result<PathBuf> {
    use std::fs::{self, OpenOptions};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use anyhow::Context;
    use windows_sys::Win32::System::Threading::DETACHED_PROCESS;

    let Some(folder) = crate::os::windows::install_location(INSTALLED_NAME)
        .context("reading where WaOS is installed")?
    else {
        anyhow::bail!(
            "WaOS is not installed: build its installer with `cargo waft build nsis -p waos-app` and run it"
        );
    };
    let log = log_path()?;
    let logs = log.parent().expect("the log is in a folder");
    fs::create_dir_all(logs).with_context(|| format!("making {}", logs.display()))?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .with_context(|| format!("opening {}", log.display()))?;
    let executable = folder.join(EXECUTABLE);
    Command::new(&executable)
        .arg(LAUNCHER_CLOSED)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log_file)
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .with_context(|| format!("starting {}", executable.display()))?;
    Ok(log)
}

/// Where a WaOS started here keeps its log: the user's local application data, under WaOS's own
/// name, where Windows apps keep theirs.
#[cfg(windows)]
fn log_path() -> Result<PathBuf> {
    use anyhow::Context;

    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .context("Windows names no LOCALAPPDATA folder to keep WaOS's log in")?;
    Ok(local.join("WaOS").join("Logs").join("WaOS.log"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn start() -> Result<PathBuf> {
    anyhow::bail!(
        "no WaOS answers, and only an installed WaOS on macOS or Linux ({BUNDLE_IDENTIFIER}), or on Windows, is known how to start: start WaOS yourself"
    )
}
