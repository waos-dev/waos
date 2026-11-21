//! WaOS's entry on a desktop, from the command line and from WaOS.app alike: CLI commands and
//! `--headless` are dispatched BEFORE `claim_the_one_instance()` and `winit`; GUI launches read
//! the command line, keep one instance of WaOS where the system keeps none itself, and show the
//! library where the command line asks. WaOS tells the desktop who it is, `dev.waos.app`, which
//! its own windows show as; a program's windows are an app of their own
//! (`logic::program_identities`).
//!
//! The invariant: developer CLI subcommands and `--headless` never touch single-instance or
//! `winit`, and every GUI launch's command line is read by `logic::command_line`'s rules.

use std::path::PathBuf;
use std::process::ExitCode;

use waft::{DefaultEmbedder, LinuxSettings, Shell, WindowsSettings};
use waft_single_instance::{LaterLaunches, SingleInstance, single_instance};
use waos::OpenRequest;
use waos_cli::{DefaultStarter, WaOSStarter};
use waos_package::ProgramId;

use crate::headless_entry;
use crate::logic::command_line::{Command, Launch, LibraryPlace, classify_launch};
use crate::logic::folders;
use crate::logic::open_command::{self, Sent};
use crate::state::{self, OpenedAtStart};
use crate::write_traces;

/// WaOS's id with the system, its bundle's identifier (`[package.metadata.waft]`), by which one
/// instance of it is kept, its Linux app id and its Windows AppUserModelID, which name its windows
/// to the desktop, as `#[waft::main]` would give them.
const APP_ID: &str = "dev.waos.app";

/// What this binary, being WaOS too, adds to the command's help: its own launches.
const APP_HELP: &str = "App commands: `waos` opens the desktop launcher, `waos --headless [--idle-timeout <SECS>]` runs the headless daemon, `waos phone` opens in a phone-sized window, `waos open <FILE>... --program <ID>` opens files with a program, and `waos <FILE>...` opens packages or files with WaOS.
The docs and guides are at https://waos.app/docs; `waos docs` prints this version's.";

/// Starter injected into `waos-cli` by the unified `waos` binary: on headless Linux (when
/// `WAYLAND_DISPLAY` and `DISPLAY` are unset, or no `.desktop` entry is installed), spawns this
/// executable itself with `--headless --idle-timeout 300`; otherwise delegates to
/// [`DefaultStarter`].
struct UnifiedStarter;

impl WaOSStarter for UnifiedStarter {
    fn start(&self) -> anyhow::Result<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            if waos_cli::waos_app::is_headless_session() || !waos_cli::waos_app::has_desktop_entry()
            {
                let exe = std::env::current_exe()
                    .map_err(|error| anyhow::anyhow!("locating the waos executable: {error}"))?;
                return waos_cli::waos_app::start_headless(
                    &exe,
                    waos_cli::waos_app::HEADLESS_IDLE_TIMEOUT_SECS,
                );
            }
        }
        DefaultStarter.start()
    }
}

/// WaOS's desktop entry: dispatches developer CLI commands and `--headless` before touching
/// single-instance or `winit`; otherwise starts the GUI app or sends `waos open` to the running
/// WaOS.
pub fn main() -> ExitCode {
    // Before anything is written: on Windows, where WaOS is a GUI program, what it writes goes to
    // the terminal it was run from, CLI and `waos open` answers among it.
    DefaultEmbedder::attach_to_parent_console();
    let launch = classify_launch(std::env::args_os()).unwrap_or_else(|usage| fail(usage));
    let command = match launch {
        Launch::Cli(argv) => return waos_cli::run_from_args(argv, &UnifiedStarter, Some(APP_HELP)),
        Launch::Headless { idle_timeout } => {
            if let Err(error) = headless_entry::run(idle_timeout) {
                fail(&format!("{error:#}"));
            }
            return ExitCode::SUCCESS;
        }
        Launch::App(command) => command,
    };

    write_traces(std::io::stderr, true);
    let (place, opened) = match command {
        Command::Show(place) => (place, None),
        // Shown at launch, as a launch by the system with a file opened with WaOS shows it.
        Command::OpenWithWaos(files) => (
            LibraryPlace::Launcher,
            Some(OpenedAtStart::WithWaos(on_the_machine(&files))),
        ),
        Command::Open { id, files } => {
            (LibraryPlace::ClosedLauncher, Some(open_or_start(id, files)))
        }
        Command::ShowProgram { id } => (
            LibraryPlace::ClosedLauncher,
            Some(OpenedAtStart::Program {
                id,
                activation_token: own_activation_token(),
            }),
        ),
    };
    let later_launches = claim_the_one_instance();
    let folders = folders::app_folders();
    let linux = LinuxSettings {
        app_id: Some(APP_ID.to_owned()),
    };
    let windows = WindowsSettings {
        app_user_model_id: Some(APP_ID.to_owned()),
    };
    let embedder = DefaultEmbedder::default().linux(linux).windows(windows);
    embedder.run(move |platform| {
        Shell::new(platform, move |app| {
            if let Err(error) = state::start(app, folders, place, opened, later_launches) {
                fail(&format!("{error:#}"));
            }
        })
    });
    ExitCode::SUCCESS
}

/// Sends `files` to the WaOS running on this machine to open with the program `id`, and exits
/// once it has them; with none running, what this WaOS opens as it starts.
fn open_or_start(id: ProgramId, files: Vec<PathBuf>) -> OpenedAtStart {
    let files = on_the_machine(&files);
    match open_command::send(&id, files.clone()) {
        Ok(Sent::Opened) => std::process::exit(0),
        Ok(Sent::NobodyAnswers) => OpenedAtStart::WithProgram {
            id,
            request: OpenRequest {
                files,
                links: Vec::new(),
            },
        },
        Err(error) => fail(&format!("{error:#}")),
    }
}

/// The activation token the desktop gave this launch on Linux (`XDG_ACTIVATION_TOKEN`, or
/// `DESKTOP_STARTUP_ID` on X11), which the window it opens takes the focus with, as
/// `waft-single-instance` reads a later launch's; none elsewhere, or where the desktop gave none.
fn own_activation_token() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|token| !token.is_empty()))
}

/// Where each of `files`, named on the command line, is on the machine; WaOS ends saying why
/// where one is not there.
fn on_the_machine(files: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .map(|file| {
            std::fs::canonicalize(file)
                .unwrap_or_else(|error| fail(&format!("{}: {error}", file.display())))
        })
        .collect()
}

/// Makes this WaOS the one instance of it, and answers the later launches, each passed on to it;
/// exits where another WaOS runs, which has this launch's command line by then. Where no claim can
/// be made, WaOS runs as though alone. macOS keeps one instance itself, so there every launch is
/// the first.
fn claim_the_one_instance() -> Option<LaterLaunches> {
    match single_instance(APP_ID) {
        Ok(SingleInstance::First(first)) => Some(first.launches()),
        Ok(SingleInstance::Second) => std::process::exit(0),
        Err(error) => {
            tracing::warn!(%error, "WaOS could not claim to be the one instance, and runs as though alone");
            None
        }
    }
}

fn fail(reason: &str) -> ! {
    eprintln!("waos: {reason}");
    std::process::exit(1)
}
