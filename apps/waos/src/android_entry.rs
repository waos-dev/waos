//! WaOS's entry on Android, which Waft's Android host calls once for the process
//! (`os::android_main`): the system's certificate checker handed the application before anything
//! connects, the engine started over the store in the app's own folders, the phone's files reached
//! as WaOS's All files access allows, and the library on the screen, laid out as a phone's, for as long as the process lives. Activities come and go under
//! it as views of the one app. Android keeps one instance of an app itself and gives
//! it no command line, so nothing here reads one.

use std::sync::Arc;

use waft::{AndroidApp, DefaultEmbedder, Shell};

use crate::logic::command_line::LibraryPlace;
use crate::logic::phone_files::PhoneFiles;
use crate::{os, state, write_traces};

/// Runs WaOS in the process `app` is, for the process's life.
pub(crate) fn main(app: AndroidApp) {
    write_traces(os::SystemLog, false);
    os::trust_the_system(&app).unwrap_or_else(|error| fail(&error.into()));
    let folders = os::app_folders(&app).unwrap_or_else(|error| fail(&error));
    let phone_files = os::AllFilesAccess::of(&app).unwrap_or_else(|error| fail(&error));
    DefaultEmbedder::default()
        .android_app(app)
        .run(move |platform| {
            Shell::new(platform, move |app| {
                let phone_files = Some(Arc::new(phone_files) as Arc<dyn PhoneFiles>);
                let started =
                    state::start(app, folders, LibraryPlace::Phone, None, None, phone_files);
                if let Err(error) = started {
                    fail(&error);
                }
            })
        });
}

/// Ends WaOS with `error` in the system's log, where an app's last words go on Android.
fn fail(error: &anyhow::Error) -> ! {
    tracing::error!("waos: {error:#}");
    std::process::exit(1)
}
