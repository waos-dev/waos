//! WaOS's entry on a desktop, from the command line and from WaOS.app alike: the command line
//! read, one instance of WaOS kept where the system keeps none itself, and the library shown where
//! the command line asks. WaOS tells the desktop who it is, `dev.waos.app`, which its own windows
//! show as; a program's windows are an app of their own (`logic::program_identities`).
//!
//! The invariant: every launch's command line is read here by `logic::command_line`'s rules, and a
//! launch that hands its files to the WaOS running starts no engine.

use std::path::PathBuf;

use waft::{DefaultEmbedder, LinuxSettings, Shell, WindowsSettings};
use waft_single_instance::{LaterLaunches, SingleInstance, single_instance};
use waos::OpenRequest;
use waos_package::ProgramId;

use crate::logic::command_line::{Command, LibraryPlace, command};
use crate::logic::folders;
use crate::logic::open_command::{self, Sent};
use crate::state::{self, OpenedAtStart};
use crate::write_traces;

/// WaOS's id with the system, its bundle's identifier (`[package.metadata.waft]`), by which one
/// instance of it is kept, its Linux app id and its Windows AppUserModelID, which name its windows
/// to the desktop, as `#[waft::main]` would give them.
const APP_ID: &str = "dev.waos.app";

/// WaOS's entry, from the command line and from WaOS.app alike: the engine started, the dev
/// server listening and the library in its launcher until the app quits, with the files the command
/// line opens with WaOS; or, for `waos open`, the files sent to the WaOS running on this machine,
/// and WaOS started here only when none is; or, for `waos open --program`, the program opened,
/// with the launch's activation token. A later launch, where WaOS keeps one instance itself, hands
/// its command line to the WaOS running, with its activation token or its right to the front, so
/// a program a pin opens comes forward.
///
/// Written out rather than emitted by `#[waft::main]`, whose entry opens the application's
/// implicit window, an ordinary one: WaOS's own window is its launcher, which it asks for itself,
/// a card with no border, hidden until it is summoned.
pub fn main() {
    // Before anything is written: on Windows, where WaOS is a GUI program, what it writes goes to
    // the terminal it was run from, `waos open`'s answers among it.
    DefaultEmbedder::attach_to_parent_console();
    let command = command(std::env::args_os().skip(1)).unwrap_or_else(|usage| fail(usage));
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
