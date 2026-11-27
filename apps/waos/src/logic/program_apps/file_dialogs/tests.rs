use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waos::bindings::wasdk::platform::views::RequestId;
use waos::{
    EngineTime, NoPrompts, ProgramEnd, ProgramEnvironment, ProgramOutput, ShellFileDialogs,
};
use waos_package::ProgramId;
use waos_window_messages::{FileDialogId, FromWindow, ToWindow};

use super::*;
use crate::logic::program_apps::app_reach::LaunchAttempt;
use crate::logic::program_apps::app_reach::tests::Launches;
use crate::logic::program_apps::awaited_answers::APP_GONE;
use crate::logic::program_apps::window_process::tests::{
    Printed, Served, next_wanted, window_process,
};
use crate::logic::test_engine::{self, NoWindows, WITHIN};

/// The id file-dialogs runs under, as an installed program's.
const FILE_DIALOGS: &str = "dev.example.filedialogs";

fn file_dialogs_id() -> ProgramId {
    FILE_DIALOGS.parse().unwrap()
}

/// File-dialogs, under its id, with no window, its dialogs opened through `reach`, as the engine
/// opens a program's on macOS, what it prints going into `printed`.
fn file_dialogs_through(reach: &Arc<AppReach>, printed: &Arc<Printed>) -> RunningProgram {
    let mut shell = test_engine::shell(Arc::new(NoWindows), Arc::new(NoPrompts));
    shell.file_dialogs = Some(Arc::clone(reach) as Arc<dyn ShellFileDialogs>);
    let engine = test_engine::engine_over(shell, EngineTime::Real);
    let file_dialogs = engine
        .load(
            Path::new(waos_test_programs::FILE_DIALOGS),
            &waos_test_programs::code_of(waos_test_programs::FILE_DIALOGS),
        )
        .expect("file-dialogs loads")
        .named(FILE_DIALOGS);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(printed) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };

    engine
        .run(&file_dialogs, environment)
        .expect("file-dialogs runs")
}

/// The file dialog the app is asked to open next: its id, the window it is a sheet on, and what
/// it is.
async fn file_dialog_asked(
    sent: &mut UnboundedReceiver<ToWindow>,
) -> (FileDialogId, Option<RequestId>, FileDialog) {
    let asked = next_wanted(sent, |message| {
        matches!(message, ToWindow::OpenFileDialog { .. })
    })
    .await;
    let ToWindow::OpenFileDialog {
        dialog,
        parent,
        kind,
    } = asked
    else {
        unreachable!("a file dialog");
    };
    (dialog, parent, kind)
}

#[tokio::test]
async fn a_file_dialog_of_a_program_with_no_app_launches_the_app_and_the_paths_picked_there_are_mounted()
 {
    let scratch = tempfile::tempdir().unwrap();
    let picks = std::fs::canonicalize(scratch.path()).unwrap();
    std::fs::create_dir_all(picks.join("pictures")).unwrap();
    std::fs::write(picks.join("pictures/one.png"), "PNG").unwrap();
    std::fs::write(picks.join("read-me.txt"), "a line to read").unwrap();
    std::fs::write(picks.join("change-me.txt"), "a line").unwrap();
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = file_dialogs_through(&reach, &printed);
    launches.until(1).await;
    assert_eq!(
        launches.asked(),
        [(file_dialogs_id(), LaunchAttempt::First)],
        "the program has no window: its first dialog launches its app"
    );

    let Served {
        process, mut sent, ..
    } = window_process(program.clone());
    reach.connected(&file_dialogs_id(), &process);
    let (dialog, parent, kind) = file_dialog_asked(&mut sent).await;
    assert_eq!(parent, None, "on its own, the program naming no view");
    assert!(
        matches!(&kind, FileDialog::OpenFile(options) if options.types[0].extensions == ["txt"] && !options.writable),
        "with what the program offers: {kind:?}"
    );
    let picked = Ok(vec![picks.join("read-me.txt")]);
    process.received(FromWindow::FileDialogClosed { dialog, picked }, Vec::new());
    let (dialog, _, kind) = file_dialog_asked(&mut sent).await;
    assert!(matches!(&kind, FileDialog::OpenFile(options) if options.writable));
    let picked = Ok(vec![picks.join("change-me.txt")]);
    process.received(FromWindow::FileDialogClosed { dialog, picked }, Vec::new());
    let (dialog, _, kind) = file_dialog_asked(&mut sent).await;
    assert!(
        matches!(&kind, FileDialog::SaveFile(options) if options.suggested_name.as_deref() == Some("saved.txt"))
    );
    let picked = Ok(vec![picks.join("saved.txt")]);
    process.received(FromWindow::FileDialogClosed { dialog, picked }, Vec::new());
    let (dialog, _, kind) = file_dialog_asked(&mut sent).await;
    assert!(matches!(kind, FileDialog::OpenFolder(_)));
    let picked = Ok(vec![picks.join("pictures")]);
    process.received(FromWindow::FileDialogClosed { dialog, picked }, Vec::new());

    let end = tokio::time::timeout(WITHIN, program.ended())
        .await
        .expect("file-dialogs ends once its dialogs are answered");
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    for line in [
        "read: a line to read",
        "read file written to: no",
        "changed: yes",
        "saved: yes",
        "folder entries: 1",
    ] {
        printed.line(line).await;
    }
    let read = |name: &str| std::fs::read_to_string(picks.join(name)).unwrap();
    assert_eq!(read("change-me.txt"), "a line, changed");
    assert_eq!(read("read-me.txt"), "a line to read");
    assert_eq!(
        launches.asked().len(),
        1,
        "every dialog after opens in that app"
    );
}

#[tokio::test]
async fn a_file_dialog_whose_app_never_comes_fails_with_why() {
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let _program = file_dialogs_through(&reach, &printed);
    launches.until(1).await;
    reach.launch_failed(&file_dialogs_id(), "Launch Services: no app");
    reach.launch_failed(&file_dialogs_id(), "Launch Services: no app");
    printed
        .line("open: failed (the program's app did not start: Launch Services: no app)")
        .await;
    launches.until(3).await;
    assert_eq!(
        launches.asked()[2],
        (file_dialogs_id(), LaunchAttempt::First),
        "the next dialog asks for the app anew"
    );
}

#[tokio::test]
async fn a_file_dialog_on_a_view_no_window_shows_opens_on_its_own_and_fails_once_the_app_goes() {
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = file_dialogs_through(&reach, &printed);
    launches.until(1).await;
    let Served {
        process, mut sent, ..
    } = window_process(program);
    reach.connected(&file_dialogs_id(), &process);
    file_dialog_asked(&mut sent).await;

    let unshown = process.open_file_dialog(Some(41), FileDialog::OpenFiles(OpenOptions::default()));
    let (_, parent, _) = file_dialog_asked(&mut sent).await;
    assert_eq!(
        parent, None,
        "a view no window shows has a dialog of its own"
    );
    process.gone();
    assert!(
        matches!(unshown.await, Err(DialogError::Failed(why)) if why == APP_GONE),
        "a dialog open as the app goes answers why nothing was picked"
    );
    printed
        .line("open: failed (the program's app is gone)")
        .await;
    printed
        .line("open writable: failed (the program's app is gone)")
        .await;
}

#[tokio::test]
async fn dropping_a_file_dialogs_future_sends_close_file_dialog_to_the_window_process() {
    let launches = Launches::default();
    let reach = launches.reach();
    let printed = Arc::new(Printed::default());
    let program = file_dialogs_through(&reach, &printed);
    launches.until(1).await;
    let Served {
        process, mut sent, ..
    } = window_process(program);
    reach.connected(&file_dialogs_id(), &process);
    file_dialog_asked(&mut sent).await;

    let open = process.open_file_dialog(None, FileDialog::OpenFiles(OpenOptions::default()));
    let (dialog_id, _, _) = file_dialog_asked(&mut sent).await;
    drop(open);
    let close = tokio::time::timeout(WITHIN, sent.next())
        .await
        .expect("CloseFileDialog sent")
        .expect("sent message");
    assert!(
        matches!(close, ToWindow::CloseFileDialog { dialog } if dialog == dialog_id),
        "{close:?}"
    );
    process.gone();
}
