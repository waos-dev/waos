//! Asking the user for a model's file, in the platform's own dialog.

use wasdk::file_dialogs::{self, OpenOptions};
use wasdk_wgpu::SharedView;

use crate::events::{AppEvent, Sender};

/// Whether the platform has a dialog to open a file with.
pub fn is_offered() -> bool {
    file_dialogs::get_file_dialogs().is_some()
}

/// Opens the dialog as a sheet on `view`'s window; the file picked comes on the app's queue.
pub fn ask(view: SharedView, sender: Sender) {
    let Some(dialogs) = file_dialogs::get_file_dialogs() else {
        return;
    };
    wasdk::spawn(async move {
        let options = OpenOptions::new();
        options.add_type(
            "glTF models",
            &["glb".to_owned(), "gltf".to_owned()],
            &["model/gltf-binary".to_owned(), "model/gltf+json".to_owned()],
        );
        match dialogs.open_file(Some(&view), options).await {
            Ok(Some(path)) => {
                let _ = sender.unbounded_send(AppEvent::FileChosen(path));
            }
            Ok(None) => {}
            Err(error) => eprintln!("model-viewer: the open dialog failed: {error}"),
        }
    });
}
