//! WaOS's entry on iOS, which the `waos` binary calls as a desktop's does: the engine started over
//! the store in the app's own container, and the library on the screen, laid out as a phone's, with
//! the programs opened from it in the deck's tabs, since iPhone gives a program no app of its own.
//! iOS keeps one instance of an app itself and gives it no command line, so nothing here reads one.

use std::process::ExitCode;

use waft::{DefaultEmbedder, Shell};

use crate::logic::command_line::LibraryPlace;
use crate::logic::folders;
use crate::{state, write_traces};

/// Runs WaOS until the system ends it.
pub fn main() -> ExitCode {
    write_traces(std::io::stderr, false);
    let folders = folders::app_folders();
    DefaultEmbedder::default().run(move |platform| {
        Shell::new(platform, move |app| {
            if let Err(error) = state::start(app, folders, LibraryPlace::Phone, None, None, None) {
                fail(&error);
            }
        })
    });
    ExitCode::SUCCESS
}

/// Ends WaOS with `error` on its standard error, which the simulator's console and Xcode's show.
fn fail(error: &anyhow::Error) -> ! {
    tracing::error!("waos: {error:#}");
    std::process::exit(1)
}
