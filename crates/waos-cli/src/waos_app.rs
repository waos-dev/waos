//! Starting the WaOS app installed on this machine, for a run when no WaOS answers, with its log
//! kept where the user's logs are: on macOS WaOS.app, which Launch Services finds; on Linux the
//! app its desktop entry starts (`desktop_entries`), or a headless `waos --headless` daemon when
//! running without a display server (`WAYLAND_DISPLAY` and `DISPLAY` unset); on Windows the app
//! its installer recorded among the installed apps.
//!
//! The invariant: a running WaOS is never started beside one that already answers on the socket.

use std::path::PathBuf;

use anyhow::Result;

/// The app's identifier, as `apps/waos`'s `[package.metadata.waft]` sets it.
#[cfg(not(windows))]
pub const BUNDLE_IDENTIFIER: &str = "dev.waos.app";

/// Default idle timeout in seconds for an auto-spawned headless Linux daemon.
#[cfg(target_os = "linux")]
pub const HEADLESS_IDLE_TIMEOUT_SECS: u64 = 300;

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

/// Whether this Linux session has no graphical display (`WAYLAND_DISPLAY` and `DISPLAY` are both
/// unset or empty).
#[cfg(target_os = "linux")]
pub fn is_headless_session() -> bool {
    let has = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    !has("WAYLAND_DISPLAY") && !has("DISPLAY")
}

/// Whether a `dev.waos.app.desktop` entry is installed on this Linux machine.
#[cfg(target_os = "linux")]
pub fn has_desktop_entry() -> bool {
    crate::desktop_entries::find(BUNDLE_IDENTIFIER).is_ok()
}

/// Spawns `executable --headless --idle-timeout <idle_timeout_secs>` in its own process group,
/// appending stdout and stderr to `$XDG_STATE_HOME/waos/waos.log`, and returns that log path.
#[cfg(target_os = "linux")]
pub fn start_headless(executable: &std::path::Path, idle_timeout_secs: u64) -> Result<PathBuf> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    use anyhow::Context;

    let log = log_path()?;
    if let Some(folder) = log.parent() {
        fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    }
    let output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .with_context(|| format!("opening {}", log.display()))?;
    Command::new(executable)
        .args([
            "--headless",
            "--idle-timeout",
            &idle_timeout_secs.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .process_group(0)
        .spawn()
        .with_context(|| format!("starting {} --headless", executable.display()))?;
    Ok(log)
}

/// Looks for a `waos` executable next to `current_exe()` or on `PATH`.
#[cfg(target_os = "linux")]
fn find_waos_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join("waos");
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("waos");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Starts WaOS on Linux: in a headless session (no `WAYLAND_DISPLAY` or `DISPLAY`), spawns `waos`
/// from `PATH` with `--headless --idle-timeout 300`; otherwise starts the desktop entry
/// `dev.waos.app.desktop` with `--launcher-closed` (falling back to headless `waos` on `PATH` if
/// no desktop entry is installed).
#[cfg(target_os = "linux")]
pub fn start() -> Result<PathBuf> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    use anyhow::{Context, bail};

    use crate::desktop_entries;

    if is_headless_session() {
        let Some(binary) = find_waos_binary() else {
            bail!(
                "no graphical display (WAYLAND_DISPLAY/DISPLAY) is set and no `waos` binary was found on PATH; install WaOS or start `waos --headless`"
            );
        };
        return start_headless(&binary, HEADLESS_IDLE_TIMEOUT_SECS);
    }

    let entry = match desktop_entries::find(BUNDLE_IDENTIFIER) {
        Ok(entry) => entry,
        Err(error) => {
            if let Some(binary) = find_waos_binary() {
                return start_headless(&binary, HEADLESS_IDLE_TIMEOUT_SECS);
            }
            return Err(error).with_context(|| {
                format!(
                    "WaOS is not installed: no {BUNDLE_IDENTIFIER}.desktop among the desktop's applications and no `waos` binary on PATH"
                )
            });
        }
    };
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
