//! Exporting a photo: a copy of its file where the user picks in the system's save dialog, a
//! sheet on the window, under the photo's own name unless they change it. WaOS mounts the place
//! picked writable, and nothing else of its folder, so the copy is made in the program's temporary
//! folder and renamed onto the place, which puts it there whole.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use waft::App;
use waft::ui::{FileDialogError, FileType, PickedFile, SaveOptions};
use waft_winui::InfoBarSeverity;

use crate::camera_app::CameraApp;
use crate::library::name_of;

/// Asks where to export the photo at `photo`, and copies it there, saying how that went.
pub(crate) fn export(app: &mut App, camera: &CameraApp, photo: &Path) {
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return camera.say(
            app,
            InfoBarSeverity::Error,
            "This system has no save dialog.",
        );
    };
    let window = platform.views().first().map(|view| view.id());
    let picking = dialogs.save_file(window, save_options(photo));
    let (camera, photo) = (camera.clone(), photo.to_owned());
    app.spawn(async move |cx| {
        let picked = picking.await;
        cx.update(|app| export_to_pick(app, &camera, &photo, picked));
    });
}

/// A save dialog that suggests `photo`'s own name, for files of its kind.
fn save_options(photo: &Path) -> SaveOptions {
    let extension = photo
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let kind = FileType {
        label: "Photo".to_owned(),
        mime_types: Vec::new(),
        extensions: vec![extension],
    };
    SaveOptions {
        types: vec![kind],
        suggested_name: Some(name_of(photo)),
        ..SaveOptions::default()
    }
}

fn export_to_pick(
    app: &mut App,
    camera: &CameraApp,
    photo: &Path,
    picked: Result<Option<PickedFile>, FileDialogError>,
) {
    let (severity, said) = match picked {
        Ok(Some(PickedFile::Path(place))) => match copy_whole(photo, &place) {
            Ok(()) => (
                InfoBarSeverity::Success,
                format!("Exported as “{}”.", name_of(&place)),
            ),
            Err(error) => (
                InfoBarSeverity::Error,
                format!("“{}” could not be written: {error}.", name_of(&place)),
            ),
        },
        Ok(Some(_)) => (
            InfoBarSeverity::Error,
            "The save dialog gave no path to write.".to_owned(),
        ),
        Ok(None) => return,
        Err(error) => (
            InfoBarSeverity::Error,
            format!("The save dialog failed: {error}."),
        ),
    };
    camera.say(app, severity, said);
}

/// Copies the file at `from` to `place` whole: into a draft in the temporary folder, then
/// renamed onto `place`; the draft goes when the rename fails.
fn copy_whole(from: &Path, place: &Path) -> io::Result<()> {
    let draft = temporary_folder().join(format!("{}.exporting", name_of(place)));
    fs::copy(from, &draft)?;
    fs::rename(&draft, place).inspect_err(|_| {
        // Nothing else of the program's reads the draft; one left behind is only clutter.
        let _ = fs::remove_file(&draft);
    })
}

/// The program's temporary folder, in its `/cache`, as `TMPDIR` names it: Rust's
/// `std::env::temp_dir` panics on `wasm32-wasip2`.
fn temporary_folder() -> PathBuf {
    std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}
