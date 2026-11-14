//! `examples/waft-menus`, an Waft app over `waft-embedder-wasdk`, under the test shell, with its
//! clipboard switches off: its `PlatformMenuBar` reaches the shell as the WIT's flat list, and a
//! choice in it reaches the item's callback, Open… asking for a file through WaOS, which it reads,
//! Open Link opening a link through WaOS's opener, and Notify asking its user and posting a
//! notification through WaOS, whose click it hears, as it hears a file and a link opened with it;
//! its text field leaves ⌘C to a host with a menu bar, and acts on Edit's select-all, copy and
//! paste, which reach it free of the switches, while under a shell without a menu bar it handles ⌘C
//! itself; its `PlatformTrayIcon`'s menu is the icon's attached menu where the icon has one, and
//! opens at the icon where it has none, a choice in either reaching its item, and a click opens a
//! context menu at its window, placed in the view's physical pixels; and a tray icon the platform
//! refuses leaves the app running without one. It is a debug build of Waft, slow to start, and
//! draws, so it runs where the engine makes hardware buffers, on Apple's systems.

use std::future::pending;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use waos::bindings::wasdk::platform::menus::{
    MenuEntry, MenuEntryKind, Provided, ProvidedItem, Submenu, TrayIconEvent,
};
use waos::bindings::wasdk::platform::notifications::Activation;
use waos::bindings::wasdk::platform::views::{EditCommand, KeyDevice, KeyEvent, KeyKind};
use waos::bindings::wasdk::platform::windows::Point;
use waos::{
    Answer, OpenRequest, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, ProgramView,
    Question, RunningProgram, WaOS,
};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

use crate::shell::clipboard::CopiedText;
use crate::shell::menus::{OpenedAt, TestMenus};
use crate::shell::opener::Opened;
use crate::shell::{self, Lines, WITHIN, resolved, wait_until};

/// The line the program's text field starts with.
const LINE: &str = "a line of waft-menus";

/// The meta key on the left, by its USB HID usage and Flutter's logical key.
const META_LEFT: (u64, u64) = (0x0007_00e3, 0x0002_0000_0106);

#[test]
fn the_menu_bar_reaches_the_shell_flat_and_a_choice_in_it_reaches_the_items_callback() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    let bar = wait_for_bar(shell.menus(), "Greet");

    let titles: Vec<_> = bar
        .iter()
        .filter(|entry| entry.parent.is_none())
        .map(label_of)
        .collect();
    assert_eq!(titles, ["Waft menus", "File", "Edit"]);
    let file = id_of(&bar, "File");
    let greet = bar
        .iter()
        .find(|entry| label_of(entry) == "Greet")
        .expect("Greet");
    assert_eq!(greet.parent, Some(file), "Greet is in File");
    let edit = id_of(&bar, "Edit");
    assert!(
        bar.iter().any(|entry| entry.parent == Some(edit)
            && matches!(
                entry.kind,
                MenuEntryKind::Provided(Provided {
                    item: ProvidedItem::Copy,
                    enabled: true
                })
            )),
        "the platform's Copy is in Edit"
    );

    running.menu_chosen(greet.id);
    lines.wait_for("menu bar: greet");
    close(&running, &window);
}

#[test]
fn its_file_dialog_goes_through_waos_whose_pick_it_reads() {
    let (engine, shell, arrivals) = shell::engine();
    let scratch = tempfile::tempdir().unwrap();
    let file = std::fs::canonicalize(scratch.path())
        .unwrap()
        .join("to-open.txt");
    std::fs::write(&file, "a line to open").unwrap();
    shell.file_dialogs().answer_next(vec![file]);
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    let bar = wait_for_bar(shell.menus(), "Open…");

    running.menu_chosen(id_of(&bar, "Open…"));
    lines.wait_for("opened: a line to open");
    let asked = shell.file_dialogs().asked();
    assert_eq!(asked[0].kind, "open-file");
    assert_eq!(asked[0].parent, Some(window.id()), "a sheet on its window");
    close(&running, &window);
}

#[test]
fn it_opens_a_link_and_posts_a_notification_through_waos_and_hears_its_click_and_what_is_opened_with_it()
 {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let grants = Arc::new(NotificationsAsked::default());
    let (running, window) = run_with(&engine, &arrivals, &lines, Some(grants));
    let bar = wait_for_bar(shell.menus(), "Open Link");

    running.menu_chosen(id_of(&bar, "Open Link"));
    lines.wait_for("opened link: ok");
    assert_eq!(
        shell.opener().opened(),
        [Opened::Link {
            program: "waft-menus".to_owned(),
            link: "https://example.com/".to_owned(),
        }]
    );

    running.menu_chosen(id_of(&bar, "Notify"));
    wait_until("the program asks", || !shell.questions().is_empty());
    assert_eq!(shell.questions()[0].1, Question::Notifications);
    shell.answer_questions(Some(Answer::Granted));
    lines.wait_for("notifications allowed: Granted");
    lines.wait_for("posted: ok");
    let shown = shell.notifications().shown();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].1.id, "greeting");
    assert_eq!(shown[0].1.body.as_deref(), Some("From waft-menus"));
    assert_eq!(shown[0].1.actions[0].id, "later");
    running.notification_activated(Activation {
        id: "greeting".to_owned(),
        action: Some("later".to_owned()),
        reply: None,
    });
    lines.wait_for(r#"notification: greeting Some("later") None"#);

    let scratch = tempfile::tempdir().unwrap();
    let file = std::fs::canonicalize(scratch.path())
        .unwrap()
        .join("opened.txt");
    std::fs::write(&file, "a line opened with it").unwrap();
    running.open_requested(OpenRequest {
        files: vec![file.clone()],
        links: vec!["https://example.com/opened".to_owned()],
    });
    lines.wait_for(&format!(
        "opened with it: {}: a line opened with it",
        file.display()
    ));
    lines.wait_for("opened with it: https://example.com/opened");
    close(&running, &window);
}

#[test]
fn its_text_field_leaves_command_c_to_the_host_and_acts_on_edits_commands() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("field: focused");

    window.edit(EditCommand::SelectAll);
    resolved(window.key(key(KeyKind::Down, META_LEFT, None)));
    let c = (0x0007_0006, u64::from('c'));
    assert!(
        !resolved(window.key(key(KeyKind::Down, c, Some("c")))),
        "⌘C is left to the host"
    );
    resolved(window.key(key(KeyKind::Up, c, None)));
    resolved(window.key(key(KeyKind::Up, META_LEFT, None)));
    assert!(
        shell.clipboard().written_types().is_empty(),
        "nothing copied"
    );

    window.copy();
    wait_until("the user's copy is written", || {
        !shell.clipboard().written_types().is_empty()
    });
    assert_eq!(
        shell.clipboard().paste(0, "text/plain").as_deref(),
        Some(LINE.as_bytes()),
        "all of the field's line, selected"
    );
    engine.set_clipboard(vec![CopiedText::item("pasted by the user")]);
    window.paste();
    lines.wait_for("field: pasted by the user");
    close(&running, &window);
}

#[test]
fn its_text_field_handles_command_c_itself_where_the_program_has_no_menu_bar() {
    let (engine, shell, arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.menu_bar = None;
    });
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("field: focused");
    window.edit(EditCommand::SelectAll);
    window.copy();
    wait_until("the user's copy is written", || {
        !shell.clipboard().written_types().is_empty()
    });

    resolved(window.key(key(KeyKind::Down, META_LEFT, None)));
    let c = (0x0007_0006, u64::from('c'));
    assert!(
        resolved(window.key(key(KeyKind::Down, c, Some("c")))),
        "the field handles ⌘C"
    );
    resolved(window.key(key(KeyKind::Up, c, None)));
    resolved(window.key(key(KeyKind::Up, META_LEFT, None)));
    close(&running, &window);
}

#[test]
fn its_tray_icons_menu_is_the_attached_one_where_the_icon_has_one_and_a_choice_reaches_its_item() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    let icon = shell.menus().wait_for_icon(1);
    assert_eq!(icon.tooltip().as_deref(), Some("Waft menus"));
    let menu = icon.wait_for_attached_menu();
    icon.choose_in_attached_menu(id_of(&menu, "Open"));
    lines.wait_for("tray menu: open");

    icon.report(TrayIconEvent::Activated(Point { x: 1.0, y: 2.0 }));
    let (at, entries) = shell.menus().wait_for_opened(1);
    assert!(
        matches!(at, OpenedAt::View { view, x, y, .. } if view == window.id() && (x, y) == (10.0, 14.0)),
        "at (5, 7) in the view's logical pixels: {at:?}"
    );
    assert_eq!(ids(&entries), [40]);
    shell.menus().choose(1, None);
    lines.wait_for("context menu chose: None");
    close(&running, &window);
    assert!(icon.is_removed(), "the icon goes with the program");
}

#[test]
fn its_tray_icons_menu_opens_at_the_icon_where_it_has_no_attached_one_and_a_choice_reaches_its_item()
 {
    let (engine, shell, arrivals) = shell::engine();
    shell.menus().without_attached_menus();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    let icon = shell.menus().wait_for_icon(1);
    // Waft's embedder takes the menu at the icon once it hears the icon's events.
    icon.wait_for_menu_at_icon_taken();

    icon.report(TrayIconEvent::ContextMenuRequested(Point {
        x: 1.0,
        y: 2.0,
    }));
    let (at, entries) = shell.menus().wait_for_opened(1);
    assert_eq!(at, OpenedAt::TrayIcon);
    shell.menus().choose(1, Some(id_of(&entries, "Open")));
    lines.wait_for("tray menu: open");
    close(&running, &window);
}

#[test]
fn a_tray_icon_the_platform_refuses_leaves_the_app_running_without_it() {
    let (engine, shell, arrivals) = shell::engine();
    shell.menus().refusing("no tray here");
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    shell.menus().wait_for_refused(1);
    assert_eq!(shell.menus().icons_added(), 0);
    close(&running, &window);
}

/// Runs `examples/waft-menus` with this computer's fonts, as an app gives them, and no switch
/// on, its output into `lines`; waits for its window.
fn run(
    engine: &WaOS,
    arrivals: &Receiver<ProgramView>,
    lines: &Arc<Lines>,
) -> (RunningProgram, ProgramView) {
    run_with(engine, arrivals, lines, None)
}

/// [`run`], with `grants`.
fn run_with(
    engine: &WaOS,
    arrivals: &Receiver<ProgramView>,
    lines: &Arc<Lines>,
    grants: Option<Arc<dyn ProgramGrants>>,
) -> (RunningProgram, ProgramView) {
    let program = shell::load(engine, waos_test_programs::WAFT_MENUS);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(lines) as Arc<dyn ProgramOutput>),
        grants,
        ..waos_local::LocalCapabilities::new().environment()
    };
    let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
    let window = arrivals
        .recv_timeout(WITHIN)
        .expect("waft-menus asks for its window");
    (running, window)
}

/// Asks the window to close, which quits the app, and waits for its end.
fn close(running: &RunningProgram, window: &ProgramView) {
    window.close_requested();
    let end = shell::wait_for_end(running);
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

/// Waits until the menu bar the program set last holds the entry titled or labelled `title`, and
/// answers that bar. Waft's `PlatformMenuBar` clears the bar before it sets its own.
fn wait_for_bar(menus: &TestMenus, title: &str) -> Vec<MenuEntry> {
    let last = || menus.bars().pop().map(|(_, bar)| bar).unwrap_or_default();
    wait_until(&format!("a menu bar with {title}"), || {
        last().iter().any(|entry| label_of(entry) == title)
    });
    last()
}

/// The id of the entry titled or labelled `title`.
fn id_of(bar: &[MenuEntry], title: &str) -> u32 {
    bar.iter()
        .find(|entry| label_of(entry) == title)
        .unwrap_or_else(|| panic!("no {title} in {bar:?}"))
        .id
}

/// A submenu's title or an item's label; nothing for another entry.
fn label_of(entry: &MenuEntry) -> &str {
    match &entry.kind {
        MenuEntryKind::Submenu(Submenu { label, .. }) => label,
        MenuEntryKind::Item(item) => &item.label,
        MenuEntryKind::Provided(_) | MenuEntryKind::Separator => "",
    }
}

fn ids(entries: &[MenuEntry]) -> Vec<u32> {
    entries.iter().map(|entry| entry.id).collect()
}

/// The key `(physical, logical)` going `kind`, typing `character`.
fn key(kind: KeyKind, (physical, logical): (u64, u64), character: Option<&str>) -> KeyEvent {
    KeyEvent {
        time: 0,
        kind,
        device: KeyDevice::Keyboard,
        physical,
        logical,
        character: character.map(str::to_owned),
        synthesized: false,
    }
}

/// Grants that give nothing but notifications, asked of the user and kept as an app keeps the
/// answer.
#[derive(Default)]
struct NotificationsAsked {
    notifications: Mutex<Option<Answer>>,
}

impl ProgramGrants for NotificationsAsked {
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
        FileSystemGrant::default()
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

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
