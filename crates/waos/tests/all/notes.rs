//! `builtin-apps/notes`, a library of notes over `wasdk-adapter-waft`, under the test shell. At
//! its first launch it asks for one window, titled `Notes`, which it keeps at least 320 by 400
//! points, under a menu bar of its own app menu, File, the platform's Edit with Search, View and
//! the platform's Window; closing the window quits it. New Note opens a note, titled `New Note`
//! until its first line is typed, and its text is kept in the program's `/data` as it is typed;
//! Search finds a note by its text, and shows it. Delete puts the note shown in Recently Deleted,
//! where View finds it and Recover takes it back. The next launch, with the same folders, shows
//! the notes kept. Import reads a text file WaOS's open dialog picks into a note, and Export writes
//! the note shown where its save dialog picks, each a sheet on the window. "Remind Me in 1 Minute"
//! asks the user for notifications, then schedules one titled with the note, and clicking it shows
//! that note. A quit is agreed to at once while every note is kept, and waits for the user's answer
//! while one cannot be. A text file opened, from the open dialog or as the system opens Notes with
//! it, is edited where it is and saved to it as it is typed, is open again at the next launch, and
//! goes from the list as it is closed. The window is titled after the note or the file shown, which
//! is how the tests see it.
//! It is a debug build of Waft, slow to start, and draws, so it runs where the engine makes
//! hardware buffers, on Apple's systems. At a phone's width, where the list is a pane over the note,
//! the same commands work, Search opening the pane.

use std::fs;
use std::future::pending;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use futures::FutureExt;
use tempfile::TempDir;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::menus::{
    MenuEntry, MenuEntryKind, Provided, ProvidedItem, ShortcutModifiers, Submenu,
};
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::bindings::wasdk::platform::views::TextInputType;
use waos::{
    Answer, OpenRequest, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramView, Question,
    RunningProgram, WaOS,
};
use waos_capabilities::{
    ChangedFuture, DomainRule, FileSystemGrant, Mount, NetworkGrant, ProgramFolders,
};

use crate::shell::clipboard::CopiedText;
use crate::shell::menus::TestMenus;
use crate::shell::{
    self, TestShell, VIEW_HEIGHT, WITHIN, resolved, wait_for_end, wait_for_presents, wait_until,
};

/// A note typed in, and its title.
const SHOPPING: &str = "Shopping\nmilk, eggs";
const SHOPPING_TITLE: &str = "Shopping";

/// A phone's width in the shell's pixels, at its 2x: 375 points.
const PHONE_WIDTH: u32 = 750;

/// How long a quit that waits for the user is watched for an answer the user never gave.
const UNANSWERED: Duration = Duration::from_secs(2);

#[test]
fn an_empty_library_opens_one_window_kept_at_its_minimum_size_under_its_menus_and_closing_it_quits()
{
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    assert_eq!(shell.requested_titles(), ["Notes"]);
    wait_for_presents(&running, &shell.placed_views()[0], 1, printed);
    let placed = Arc::clone(&shell.placed_windows()[0]);
    wait_until("the window keeps its minimum size", || {
        placed.min_size() == Some((320.0, 400.0))
    });
    wait_for_title(&shell, "Notes");
    assert!(kept_notes(&folders).is_empty(), "an empty library");

    let bar = wait_for_bar(shell.menus());
    let titles: Vec<_> = bar
        .iter()
        .filter(|entry| entry.parent.is_none())
        .map(label_of)
        .collect();
    assert_eq!(titles, ["Notes", "File", "Edit", "View", "Window"]);
    assert_eq!(
        labels_in(&bar, "File"),
        [
            "New Note",
            "Open…",
            "Close File",
            "Import…",
            "Export…",
            "Delete",
            "Recover",
            "Remind Me in 1 Minute",
            "Close"
        ]
    );
    assert_eq!(
        shortcut_modifiers(&bar, "File", "Delete"),
        ShortcutModifiers::META
    );
    assert!(
        provides(&bar, "Edit", ProvidedItem::Paste),
        "Edit is the platform's"
    );
    assert_eq!(labels_in(&bar, "Edit"), ["Search"], "and Search");
    assert_eq!(labels_in(&bar, "View"), ["Notes", "Recently Deleted"]);
    assert!(
        provides(&bar, "Window", ProvidedItem::Minimize),
        "Window is the platform's"
    );

    window.close_requested();
    let end = wait_for_end(&running);
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "closing its window quits it, not {end:?}"
    );
}

#[test]
fn a_new_note_is_kept_as_it_is_typed_and_found_by_its_text() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());

    type_new_note(&engine, &shell, &running, &window, SHOPPING, SHOPPING_TITLE);
    wait_until("the note is kept as it is typed", || {
        kept_notes(&folders) == [kept(SHOPPING)]
    });
    running.menu_chosen(id_of(&bar, "File", "New Note"));
    wait_for_title(&shell, "New Note");

    running.menu_chosen(id_of(&bar, "Edit", "Search"));
    paste(&engine, &window, "eggs");
    wait_for_title(&shell, SHOPPING_TITLE);
    close(&running, &window);
}

#[test]
fn a_deleted_note_goes_to_recently_deleted_and_is_recovered_from_there() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());
    type_new_note(&engine, &shell, &running, &window, "Plans", "Plans");

    running.menu_chosen(id_of(&bar, "File", "Delete"));
    wait_for_title(&shell, "Notes");
    wait_until("the note is kept as deleted", || {
        kept_notes(&folders)
            .first()
            .is_some_and(|note| note.deleted)
    });
    running.menu_chosen(id_of(&bar, "View", "Recently Deleted"));
    wait_for_title(&shell, "Plans");

    running.menu_chosen(id_of(&bar, "File", "Recover"));
    wait_until("the note is kept as recovered", || {
        kept_notes(&folders) == [kept("Plans")]
    });
    running.menu_chosen(id_of(&bar, "View", "Recently Deleted"));
    wait_for_title(&shell, "Notes");
    running.menu_chosen(id_of(&bar, "View", "Notes"));
    wait_for_title(&shell, "Plans");
    close(&running, &window);
}

#[test]
fn at_a_phones_width_the_list_is_a_pane_over_the_note_that_search_opens() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());
    window.metrics_changed(shell::metrics(PHONE_WIDTH, VIEW_HEIGHT));

    type_new_note(&engine, &shell, &running, &window, SHOPPING, SHOPPING_TITLE);
    running.menu_chosen(id_of(&bar, "File", "New Note"));
    wait_for_title(&shell, "New Note");
    let started = shell.placed_views()[0].text_inputs_started().len();
    running.menu_chosen(id_of(&bar, "Edit", "Search"));
    wait_until(
        "the search field has the focus, once the pane has opened",
        || {
            let inputs = shell.placed_views()[0].text_inputs_started();
            inputs.len() > started && matches!(inputs.last(), Some(TextInputType::Text))
        },
    );
    paste(&engine, &window, "milk");
    wait_for_title(&shell, SHOPPING_TITLE);
    close(&running, &window);
}

#[test]
fn the_notes_kept_are_there_at_the_next_launch() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    type_new_note(&engine, &shell, &running, &window, SHOPPING, SHOPPING_TITLE);
    assert_eq!(
        resolved(running.request_exit()),
        AppExitResponse::Exit,
        "every note is kept, so nothing keeps it"
    );
    running.stop();
    wait_for_end(&running);

    let (engine, shell, arrivals) = shell::engine();
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    assert_eq!(shell.requested_titles(), ["Notes"]);
    wait_for_title(&shell, SHOPPING_TITLE);
    close(&running, &window);
}

#[test]
fn import_reads_a_picked_text_file_into_a_note_and_export_writes_the_note_where_waos_mounts_the_pick()
 {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let scratch = TempDir::new().unwrap();
    let scratch_path = fs::canonicalize(scratch.path()).unwrap();
    let (recipe, exported) = (
        scratch_path.join("recipe.txt"),
        scratch_path.join("pancakes.txt"),
    );
    let pancakes = "Pancakes\nflour, eggs, milk";
    fs::write(&recipe, pancakes).unwrap();
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());

    shell.file_dialogs().answer_next(vec![recipe]);
    running.menu_chosen(id_of(&bar, "File", "Import…"));
    wait_for_title(&shell, "Pancakes");
    assert_eq!(kept_notes(&folders), [kept(pancakes)]);

    shell.file_dialogs().answer_next(vec![exported.clone()]);
    running.menu_chosen(id_of(&bar, "File", "Export…"));
    wait_until("the note is exported", || {
        fs::read_to_string(&exported).is_ok_and(|text| text == pancakes)
    });
    let asked = shell.file_dialogs().asked();
    assert_eq!(asked[0].kind, "open-files");
    assert_eq!(asked[1].kind, "save-file");
    assert_eq!(asked[1].suggested_name.as_deref(), Some("Pancakes.txt"));
    assert!(
        asked
            .iter()
            .all(|dialog| dialog.parent == Some(window.id())),
        "each a sheet on the window"
    );
    close(&running, &window);
}

#[test]
fn a_reminder_is_scheduled_a_minute_later_with_the_notes_title_and_its_click_shows_the_note() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());
    type_new_note(&engine, &shell, &running, &window, "Call Mum", "Call Mum");

    let asked_at = SystemTime::now();
    running.menu_chosen(id_of(&bar, "File", "Remind Me in 1 Minute"));
    wait_until("the program asks", || !shell.questions().is_empty());
    assert_eq!(shell.questions()[0].1, Question::Notifications);
    shell.answer_questions(Some(Answer::Granted));
    wait_until("the reminder is scheduled", || {
        !shell.notifications().scheduled().is_empty()
    });
    let seen_at = SystemTime::now();
    let scheduled = shell.notifications().scheduled().remove(0);
    assert_eq!(scheduled.notification.title, "Call Mum");
    assert!(
        scheduled.notification.id.starts_with("note:"),
        "{}",
        scheduled.notification.id
    );
    // The program reads its clock somewhere between the choice and the test seeing the reminder.
    let a_minute = Duration::from_secs(60);
    let slack = Duration::from_secs(1);
    let earliest = asked_at + a_minute - slack;
    let latest = seen_at + a_minute + slack;
    assert!(
        (earliest..latest).contains(&scheduled.at),
        "a minute later: {:?} after the choice, {:?} after it was seen",
        scheduled.at.duration_since(asked_at),
        scheduled.at.duration_since(seen_at),
    );

    running.menu_chosen(id_of(&bar, "File", "New Note"));
    wait_for_title(&shell, "New Note");
    running.notification_activated(Activation {
        id: scheduled.notification.id,
        action: None,
        reply: None,
    });
    wait_for_title(&shell, "Call Mum");
    close(&running, &window);
}

#[test]
fn a_quit_while_a_note_cannot_be_saved_waits_for_the_users_answer() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let (running, _window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());
    let store = folders.notes();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o555)).unwrap();

    running.menu_chosen(id_of(&bar, "File", "New Note"));
    wait_for_title(&shell, "New Note");
    assert!(kept_notes(&folders).is_empty(), "the store cannot keep it");
    let (answer, answered) = mpsc::channel();
    let asked = running.request_exit();
    thread::spawn(move || {
        // A test that is over has dropped the receiver.
        let _ = answer.send(futures::executor::block_on(asked));
    });
    assert!(
        answered.recv_timeout(UNANSWERED).is_err(),
        "a quit waits while the window asks whether to lose the note"
    );
    assert!(
        running.ended().now_or_never().is_none(),
        "it runs, asking: {:#?}",
        printed()
    );
    fs::set_permissions(&store, fs::Permissions::from_mode(0o755)).unwrap();
    running.stop();
    wait_for_end(&running);
}

#[test]
fn a_text_file_opened_from_the_open_dialog_is_edited_where_it_is_and_closed_from_the_list() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let todo = TextFile::new("todo.txt", "milk");
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());
    wait_for_presents(&running, &shell.placed_views()[0], 1, printed);

    shell.file_dialogs().answer_next(vec![todo.path.clone()]);
    running.menu_chosen(id_of(&bar, "File", "Open…"));
    wait_for_title(&shell, "todo.txt");
    paste(&engine, &window, "eggs, ");
    wait_until("the file is saved where it is as it is typed", || {
        todo.text() == "eggs, milk"
    });
    let asked = shell.file_dialogs().asked();
    assert_eq!(asked[0].kind, "open-files");
    assert_eq!(asked[0].parent, Some(window.id()), "a sheet on the window");
    assert!(kept_notes(&folders).is_empty(), "the file is no note");

    running.menu_chosen(id_of(&bar, "File", "Close File"));
    wait_for_title(&shell, "Notes");
    assert_eq!(todo.text(), "eggs, milk", "left as it was saved");
    close(&running, &window);
}

#[test]
fn a_text_file_the_system_opens_notes_with_arrives_through_an_open_request() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let letter = TextFile::new("letter.txt", "Dear Sam,");
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    type_new_note(&engine, &shell, &running, &window, SHOPPING, SHOPPING_TITLE);

    running.open_requested(OpenRequest {
        files: vec![letter.path.clone()],
        links: Vec::new(),
    });
    wait_for_title(&shell, "letter.txt");
    paste(&engine, &window, "Hello. ");
    wait_until("the file is saved where it is as it is typed", || {
        letter.text() == "Hello. Dear Sam,"
    });
    assert_eq!(
        kept_notes(&folders),
        [kept(SHOPPING)],
        "the note is untouched"
    );
    close(&running, &window);
}

#[test]
fn a_text_file_open_is_open_again_at_the_next_launch() {
    let (engine, shell, arrivals) = shell::engine();
    let folders = Folders::new();
    let grants = Arc::new(NotesGrants::default());
    let todo = TextFile::new("todo.txt", "milk");
    let (running, _window) = launch(&engine, &arrivals, &folders, &grants);
    let bar = wait_for_bar(shell.menus());
    shell.file_dialogs().answer_next(vec![todo.path.clone()]);
    running.menu_chosen(id_of(&bar, "File", "Open…"));
    wait_for_title(&shell, "todo.txt");
    assert_eq!(
        resolved(running.request_exit()),
        AppExitResponse::Exit,
        "the file is saved, so nothing keeps it"
    );
    running.stop();
    wait_for_end(&running);

    let (engine, shell, arrivals) = shell::engine();
    let (running, window) = launch(&engine, &arrivals, &folders, &grants);
    wait_for_title(&shell, "todo.txt");
    assert!(
        grants
            .picked
            .lock()
            .unwrap()
            .iter()
            .any(|mount| mount.at == todo.path && mount.writable),
        "WaOS keeps it mounted writable, where Notes reads it again"
    );
    close(&running, &window);
}

/// Launches `builtin-apps/notes` with this computer's fonts, as an app gives them, `folders` as its
/// own and `grants`; waits for its window.
fn launch(
    engine: &WaOS,
    arrivals: &Receiver<ProgramView>,
    folders: &Folders,
    grants: &Arc<NotesGrants>,
) -> (RunningProgram, ProgramView) {
    shell::output();
    let program = shell::load(engine, waos_test_programs::NOTES);
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(grants) as Arc<dyn ProgramGrants>),
        folders: Some(folders.own()),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let window = arrivals
        .recv_timeout(WITHIN)
        .expect("notes asks for its window");
    (running, window)
}

/// Closes the window, which quits the program, and waits for it to end.
fn close(running: &RunningProgram, window: &ProgramView) {
    window.close_requested();
    wait_for_end(running);
}

/// What every run of the program printed so far, for a failure to show.
fn printed() -> Vec<String> {
    shell::output().lines_of(0, "notes")
}

/// Makes a note with New Note, once the window has drawn, and pastes `text` into it, waiting until
/// the window is titled after it, `title`.
fn type_new_note(
    engine: &WaOS,
    shell: &TestShell,
    running: &RunningProgram,
    window: &ProgramView,
    text: &str,
    title: &str,
) {
    wait_for_presents(running, &shell.placed_views()[0], 1, printed);
    running.menu_chosen(id_of(&wait_for_bar(shell.menus()), "File", "New Note"));
    wait_for_title(shell, "New Note");
    paste(engine, window, text);
    wait_for_title(shell, title);
}

/// Pastes `text` where the window's focus is.
fn paste(engine: &WaOS, window: &ProgramView, text: &str) {
    engine.set_clipboard(vec![CopiedText::item(text)]);
    window.paste();
}

/// Waits until the window is titled `title`.
fn wait_for_title(shell: &TestShell, title: &str) {
    let window = Arc::clone(&shell.placed_windows()[0]);
    wait_until(&format!("the window is titled {title}"), || {
        window.titles().last().map(String::as_str) == Some(title)
    });
}

/// A note as the program's store keeps it.
#[derive(Debug, PartialEq)]
struct KeptNote {
    text: String,
    deleted: bool,
}

/// A note kept with `text`, among the notes.
fn kept(text: &str) -> KeptNote {
    KeptNote {
        text: text.to_owned(),
        deleted: false,
    }
}

/// The notes the program's store keeps now, read from their files.
fn kept_notes(folders: &Folders) -> Vec<KeptNote> {
    let Ok(files) = fs::read_dir(folders.notes()) else {
        return Vec::new();
    };
    files
        .map(|file| file.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let file: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            KeptNote {
                text: file["text"].as_str().unwrap().to_owned(),
                deleted: !file["deleted"].is_null(),
            }
        })
        .collect()
}

/// Waits until the program has set its menu bar, and answers it. Waft's `PlatformMenuBar` clears
/// the bar before it sets its own.
fn wait_for_bar(menus: &TestMenus) -> Vec<MenuEntry> {
    let last = || menus.bars().pop().map(|(_, bar)| bar).unwrap_or_default();
    wait_until("a menu bar set", || !last().is_empty());
    last()
}

/// The id of the item labelled `label` in the menu titled `menu`.
fn id_of(bar: &[MenuEntry], menu: &str, label: &str) -> u32 {
    item(bar, menu, label).id
}

fn item<'a>(bar: &'a [MenuEntry], menu: &str, label: &str) -> &'a MenuEntry {
    let menu = top_level(bar, menu);
    bar.iter()
        .find(|entry| entry.parent == Some(menu) && label_of(entry) == label)
        .unwrap_or_else(|| panic!("no {label} in {bar:?}"))
}

/// The labels of the program's own items in the menu titled `menu`, in order.
fn labels_in<'a>(bar: &'a [MenuEntry], menu: &str) -> Vec<&'a str> {
    let menu = top_level(bar, menu);
    bar.iter()
        .filter(|entry| entry.parent == Some(menu))
        .map(label_of)
        .filter(|label| !label.is_empty())
        .collect()
}

/// The id of the menu titled `menu` in the bar itself.
fn top_level(bar: &[MenuEntry], menu: &str) -> u32 {
    bar.iter()
        .find(|entry| entry.parent.is_none() && label_of(entry) == menu)
        .unwrap_or_else(|| panic!("no menu {menu} in {bar:?}"))
        .id
}

/// Whether the menu titled `menu` holds the platform's `item`.
fn provides(bar: &[MenuEntry], menu: &str, item: ProvidedItem) -> bool {
    let menu = top_level(bar, menu);
    bar.iter().any(|entry| {
        entry.parent == Some(menu)
            && matches!(entry.kind, MenuEntryKind::Provided(Provided { item: provided, .. }) if provided == item)
    })
}

/// The modifier keys of the shortcut of the item labelled `label` in the menu titled `menu`.
fn shortcut_modifiers(bar: &[MenuEntry], menu: &str, label: &str) -> ShortcutModifiers {
    match &item(bar, menu, label).kind {
        MenuEntryKind::Item(item) => item.shortcut.as_ref().expect("a shortcut").modifiers,
        kind => panic!("{label} is no item of the program's own: {kind:?}"),
    }
}

/// A submenu's title or an item's label; nothing for another entry.
fn label_of(entry: &MenuEntry) -> &str {
    match &entry.kind {
        MenuEntryKind::Submenu(Submenu { label, .. }) => label,
        MenuEntryKind::Item(item) => &item.label,
        MenuEntryKind::Provided(_) | MenuEntryKind::Separator => "",
    }
}

/// A text file somewhere on the machine, outside the program's own folders, as the user has one.
struct TextFile {
    /// Its path, with every link resolved, which is where WaOS mounts it.
    path: PathBuf,
    _folder: TempDir,
}

impl TextFile {
    /// A file named `name` holding `text`, in a folder made for the test.
    fn new(name: &str, text: &str) -> TextFile {
        let folder = TempDir::new().unwrap();
        let path = fs::canonicalize(folder.path()).unwrap().join(name);
        fs::write(&path, text).unwrap();
        TextFile {
            path,
            _folder: folder,
        }
    }

    /// What it holds now.
    fn text(&self) -> String {
        fs::read_to_string(&self.path).unwrap_or_default()
    }
}

/// The program's own folders, made for the test, kept from one launch to the next.
struct Folders {
    root: TempDir,
}

impl Folders {
    fn new() -> Folders {
        let root = TempDir::new().unwrap();
        for folder in ["data", "cache"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        Folders { root }
    }

    fn own(&self) -> ProgramFolders {
        let at = |folder: &str| self.root.path().join(folder);
        ProgramFolders {
            data: at("data"),
            cache: at("cache"),
        }
    }

    /// Where the program keeps its notes: its `/data/notes`.
    fn notes(&self) -> PathBuf {
        self.root.path().join("data").join("notes")
    }
}

/// Grants as an app keeps them for the program: no switch on, each file picked mounted again at
/// the next launch, and the user's answer to notifications kept.
#[derive(Default)]
struct NotesGrants {
    picked: Mutex<Vec<Mount>>,
    notifications: Mutex<Option<Answer>>,
}

impl ProgramGrants for NotesGrants {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Any,
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant {
            mounts: self.picked.lock().unwrap().clone(),
        }
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        match question {
            Question::Notifications => *self.notifications.lock().unwrap(),
            Question::Camera | Question::Microphone => Some(Answer::Denied),
        }
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        if question == Question::Notifications {
            *self.notifications.lock().unwrap() = Some(answer);
        }
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn keep_picked(&self, mount: Mount) {
        self.picked.lock().unwrap().push(mount);
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
