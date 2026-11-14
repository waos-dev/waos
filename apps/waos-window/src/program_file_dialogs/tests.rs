use std::path::PathBuf;
use std::time::Duration;

use futures::channel::mpsc;
use waft::Shell;
use waft::ui::EmbedderClient;
use waft_test::{AskedDialog, MockFileDialogs, TestPlatform};
use waos::{FileType, FolderOptions, OpenOptions};

use super::*;
use crate::connection::tests::{connection, sent};

/// An open dialog offering text files.
fn text_file() -> FileDialog {
    FileDialog::OpenFile(OpenOptions {
        types: vec![FileType {
            label: "Text".to_owned(),
            extensions: vec!["txt".to_owned()],
            mime_types: vec!["text/plain".to_owned()],
        }],
        ..OpenOptions::default()
    })
}

#[test]
fn a_dialog_opens_on_the_view_it_is_asked_for_and_the_paths_picked_go_to_waos_once_it_closes() {
    let picked = PathBuf::from("/Users/someone/Notes/a note.txt");
    let dialogs = MockFileDialogs::new().answering_paths(vec![picked.clone()]);
    let platform = TestPlatform::new().with_file_dialogs(Rc::new(dialogs.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let parent = ViewId(3);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let shown = show(app, Some(parent), text_file());
        answer_when_closed(app, connection, 7, shown);
    });
    shell.wake(Duration::ZERO);
    let asked = dialogs.asked();
    assert!(
        matches!(
            &asked[..],
            [AskedDialog::OpenFile(Some(view), options)]
                if *view == parent && options.types[0].extensions == ["txt"]
        ),
        "a sheet on the view, offering what the program offers: {asked:?}"
    );
    let closed = sent(&waos);
    assert!(
        matches!(
            &closed,
            FromWindow::FileDialogClosed { dialog: 7, picked: Ok(paths) } if *paths == [picked]
        ),
        "{closed:?}"
    );
}

#[test]
fn a_dialog_on_its_own_that_the_user_cancels_goes_to_waos_with_no_paths() {
    let dialogs = MockFileDialogs::new();
    let platform = TestPlatform::new().with_file_dialogs(Rc::new(dialogs.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let (_requests, requested) = mpsc::unbounded();
        let view_windows = ViewWindows::<WaosProgram>::listen(app, requested);
        let asked = AskedFileDialog {
            dialog: 8,
            parent: None,
            kind: FileDialog::OpenFolder(FolderOptions::default()),
        };
        open_file_dialog(app, &view_windows, connection, asked);
    });
    shell.wake(Duration::ZERO);
    assert!(
        matches!(&dialogs.asked()[..], [AskedDialog::OpenFolder(None, _)]),
        "on its own"
    );
    let closed = sent(&waos);
    assert!(
        matches!(
            &closed,
            FromWindow::FileDialogClosed { dialog: 8, picked: Ok(paths) } if paths.is_empty()
        ),
        "{closed:?}"
    );
}

#[test]
fn a_dialog_on_a_window_no_longer_shown_opens_on_its_own() {
    let dialogs = MockFileDialogs::new().answering_paths(vec![PathBuf::from("/tmp/a.txt")]);
    let platform = TestPlatform::new().with_file_dialogs(Rc::new(dialogs.clone()));
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let mut shell = Shell::new(Rc::new(platform), |app| {
        let (_requests, requested) = mpsc::unbounded();
        let view_windows = ViewWindows::<WaosProgram>::listen(app, requested);
        let asked = AskedFileDialog {
            dialog: 9,
            parent: Some(2),
            kind: text_file(),
        };
        open_file_dialog(app, &view_windows, connection, asked);
    });
    shell.wake(Duration::ZERO);
    assert!(
        matches!(&dialogs.asked()[..], [AskedDialog::OpenFile(None, _)]),
        "on its own: {:?}",
        dialogs.asked()
    );
    let closed = sent(&waos);
    assert!(
        matches!(
            &closed,
            FromWindow::FileDialogClosed { dialog: 9, picked: Ok(paths) } if *paths == [PathBuf::from("/tmp/a.txt")]
        ),
        "{closed:?}"
    );
}

#[test]
fn a_dialog_where_the_host_has_none_is_answered_at_once_with_why() {
    let (connection, waos) = connection();
    let connection = Rc::new(connection);
    let _shell = Shell::new(Rc::new(TestPlatform::new()), |app| {
        let (_requests, requested) = mpsc::unbounded();
        let view_windows = ViewWindows::<WaosProgram>::listen(app, requested);
        let asked = AskedFileDialog {
            dialog: 10,
            parent: None,
            kind: text_file(),
        };
        open_file_dialog(app, &view_windows, connection, asked);
    });
    let closed = sent(&waos);
    assert!(
        matches!(
            &closed,
            FromWindow::FileDialogClosed {
                dialog: 10,
                picked: Err(DialogError::Failed(_))
            }
        ),
        "{closed:?}"
    );
}
