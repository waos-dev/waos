//! `examples/menus` under the test shell, with its clipboard switches off: each key it is sent
//! answers whether it handled it, as the program replied, a reply it dropped unanswered counting
//! as not handled; the user's copy and cut write once each, and the user's paste reads, whatever
//! the switches say; a file the user pasted is mounted read-only at its own path, where the
//! program reads it and cannot write to it; Edit's other commands reach the view; the menu bar it
//! sets is kept and told to the shell, and a choice in it reaches it; a context menu opens at its
//! view and answers the choice; its tray icon, once added, says the menus it offers, or why the
//! platform refused it, reports what the user does to it, to every stream of its events, its menus
//! answer the choices, and it goes when the program ends; an event on the
//! icon of a frozen program thaws
//! it until it has handled the event; and a shell without menus gives the program none.
//!
//! Freezing is the app's suspension, on time the test moves (`EngineTime::Manual`): a suspended
//! program is frozen at once, and again as soon as it has answered what thawed it, which the test
//! sees as it happens (`RunningProgram::frozen`); and a frozen program woken by an event stands at
//! its next epoch check before it hears the event (`RunningProgram::stopped_at_freeze`,
//! `freezing`), once a tick has passed since it last ran.

use std::fs;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use waos::bindings::wasdk::platform::menus::{
    MenuEntry, MenuEntryKind, MenuItem, Shortcut, ShortcutModifiers, TrayIconEvent,
};
use waos::bindings::wasdk::platform::views::{EditCommand, KeyDevice, KeyEvent, KeyKind};
use waos::bindings::wasdk::platform::windows::Point;
use waos::{
    FreezePolicy, ManualTime, ProgramEnd, ProgramEnvironment, ProgramOutput, ProgramView,
    RunningProgram, WaOS,
};

use crate::shell::clipboard::{CopiedFile, CopiedText};
use crate::shell::menus::OpenedAt;
use crate::shell::{
    self, Lines, WITHIN, resolved, wait_until_frozen, wait_until_stopped_at_freeze,
};

/// The engine's tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

#[test]
fn a_key_answers_whether_the_program_handled_it() {
    let (engine, _shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);

    assert!(resolved(window.key(key_down('a'))), "handled");
    lines.wait_for("key a: handled");
    assert!(
        !resolved(window.key(key_down('b'))),
        "its reply dropped unanswered"
    );
    lines.wait_for("key b: left");
    assert!(!resolved(window.key(key_down('c'))), "answered no");
    lines.wait_for("key 0x63: not handled");

    window.close_requested();
    assert!(matches!(
        shell::wait_for_end(&running),
        ProgramEnd::Returned(Ok(()))
    ));
}

#[test]
fn the_users_copy_cut_and_paste_are_free_of_the_switches() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);

    engine.set_clipboard(vec![CopiedText::item("from another app")]);
    window.paste();
    lines.wait_for("paste: from another app");

    window.copy();
    lines.wait_for("copy: written");
    lines.wait_for("copy again: denied");
    assert_eq!(
        shell.clipboard().paste(0, "text/plain").as_deref(),
        Some(&b"copied by menus"[..])
    );
    window.cut();
    lines.wait_for("cut: written");
    lines.wait_for("cut again: denied");
    assert_eq!(
        shell.clipboard().paste(0, "text/plain").as_deref(),
        Some(&b"cut by menus"[..])
    );

    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn a_file_the_user_pasted_is_read_and_not_written() {
    let (engine, _shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    let folder = tempfile::tempdir().unwrap();
    let file = fs::canonicalize(folder.path()).unwrap().join("pasted.txt");
    fs::write(&file, "a pasted line").unwrap();

    engine.set_clipboard(vec![CopiedFile::item(&file)]);
    window.paste();
    lines.wait_for("pasted file: a pasted line");
    lines.wait_for("pasted file written to: no");
    assert_eq!(fs::read_to_string(&file).unwrap(), "a pasted line");

    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn edits_other_commands_reach_the_view() {
    let (engine, _shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    for command in [
        EditCommand::Undo,
        EditCommand::Redo,
        EditCommand::SelectAll,
        EditCommand::Delete,
    ] {
        window.edit(command);
        lines.wait_for(&format!("edit: {command:?}"));
    }
    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn the_menu_bar_a_program_sets_is_kept_and_told_to_the_shell_and_a_choice_in_it_reaches_it() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("menu bar: set");

    let bars = shell.menus().bars();
    assert_eq!(bars.len(), 1, "set once: {bars:?}");
    let (program, told) = &bars[0];
    assert_eq!(program, "menus");
    let kept = running.menu_bar().expect("the bar is kept");
    assert_eq!(
        format!("{told:?}"),
        format!("{kept:?}"),
        "the shell is told the bar kept"
    );
    assert_eq!(ids(&kept), [1, 2, 3, 4, 10, 11, 12, 20, 21, 22, 23]);
    assert!(
        matches!(
            &kept[5].kind,
            MenuEntryKind::Item(MenuItem {
                shortcut: Some(Shortcut { logical: 0x67, modifiers }),
                ..
            }) if *modifiers == ShortcutModifiers::META
        ),
        "Greet, under ⌘G: {:?}",
        kept[5]
    );

    running.menu_chosen(11);
    lines.wait_for("menu bar chose: 11");
    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn a_context_menu_opens_at_the_view_and_answers_the_item_chosen_or_none() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);

    assert!(resolved(window.key(key_down('m'))), "handled");
    let (at, entries) = shell.menus().wait_for_opened(1);
    assert_eq!(
        at,
        OpenedAt::View {
            program: "menus".into(),
            view: window.id(),
            x: 10.0,
            y: 20.0,
        }
    );
    assert_eq!(ids(&entries), [40, 41]);
    shell.menus().choose(1, Some(40));
    lines.wait_for("context menu chose: Some(40)");

    assert!(resolved(window.key(key_down('m'))));
    shell.menus().wait_for_opened(2);
    shell.menus().choose(2, None);
    lines.wait_for("context menu chose: None");

    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn a_tray_icon_reports_what_the_user_does_answers_its_menus_and_goes_when_the_program_ends() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("tray icon: added");
    assert!(lines.has("tray icon menus: attached yes, at icon yes"));
    let icon = shell.menus().wait_for_icon(1);
    assert_eq!(icon.program, "menus");
    assert_eq!(icon.tooltip().as_deref(), Some("Menus"));
    let image = icon.image();
    assert_eq!((image.width, image.height, image.template), (1, 1, true));
    assert_eq!(ids(&icon.wait_for_attached_menu()), [30, 31]);

    icon.report(TrayIconEvent::Activated(Point { x: 1.0, y: 2.0 }));
    lines.wait_for("tray: activated at 1,2");
    icon.choose_in_attached_menu(30);
    lines.wait_for("attached menu chose: 30");
    icon.report(TrayIconEvent::ContextMenuRequested(Point {
        x: 1.0,
        y: 2.0,
    }));
    let (at, entries) = shell.menus().wait_for_opened(1);
    assert_eq!(at, OpenedAt::TrayIcon);
    assert_eq!(ids(&entries), [30, 31]);
    shell.menus().choose(1, Some(31));
    lines.wait_for("menu at icon chose: Some(31)");
    assert!(!icon.is_removed());

    window.close_requested();
    shell::wait_for_end(&running);
    assert!(icon.is_removed(), "the icon goes with the program");
}

#[test]
fn a_tray_icon_says_the_menus_the_platform_made_it_with_and_a_refused_one_why() {
    let (engine, shell, arrivals) = shell::engine();
    shell.menus().without_attached_menus();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("tray icon menus: attached no, at icon yes");
    window.close_requested();
    shell::wait_for_end(&running);

    let (engine, shell, arrivals) = shell::engine();
    shell.menus().refusing("no tray here");
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("tray icon: failed (no tray here)");
    assert_eq!(shell.menus().icons_added(), 0);
    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn every_stream_of_a_tray_icons_events_hears_each_event() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("tray icon: added");
    let icon = shell.menus().wait_for_icon(1);

    icon.report(TrayIconEvent::Activated(Point { x: 3.0, y: 4.0 }));
    lines.wait_for("tray: activated at 3,4");
    lines.wait_for("tray, heard again: activated at 3,4");
    window.close_requested();
    shell::wait_for_end(&running);
}

#[test]
fn an_event_on_a_frozen_programs_tray_icon_thaws_it_until_it_has_handled_the_event() {
    let time = ManualTime::new();
    let only_when_suspended = FreezePolicy { after_hidden: None };
    let (engine, shell, arrivals) = shell::engine_on(&time, only_when_suspended);
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    lines.wait_for("tray icon: added");
    let icon = shell.menus().wait_for_icon(1);
    resolved(engine.app_suspending());
    edit_while_frozen(&running, &time, &window, EditCommand::Undo);
    assert!(!heard(&lines, "edit: EditCommand::Undo"), "frozen");

    icon.report(TrayIconEvent::ContextMenuRequested(Point {
        x: 1.0,
        y: 2.0,
    }));
    lines.wait_for("tray: menu asked for");
    lines.wait_for("edit: EditCommand::Undo");
    shell.menus().wait_for_opened(1);
    shell.menus().choose(1, Some(30));
    lines.wait_for("menu at icon chose: Some(30)");

    // Its next read of the icon's events lets the thaw go: from then on it is frozen.
    edit_while_frozen(&running, &time, &window, EditCommand::Redo);
    assert!(
        !heard(&lines, "edit: EditCommand::Redo"),
        "handled, it is frozen again"
    );
    engine.app_resumed();
    lines.wait_for("edit: EditCommand::Redo");
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_shell_without_menus_gives_the_program_none() {
    let (engine, _shell, arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.menu_bar = None;
        parts.context_menus = None;
        parts.tray_icons = None;
    });
    let lines = Arc::new(Lines::default());
    let (running, window) = run(&engine, &arrivals, &lines);
    for line in ["menu bar: none", "tray icons: none", "context menus: none"] {
        lines.wait_for(line);
    }
    assert_eq!(running.menu_bar().map(|bar| bar.len()), None);
    window.close_requested();
    shell::wait_for_end(&running);
}

/// Once the program is frozen, moves its time a tick past the moment it last ran, then sends its
/// window `command`, which wakes it, and waits until it stands at its freeze: at the epoch check
/// it reaches before it can hear the command.
fn edit_while_frozen(
    running: &RunningProgram,
    time: &ManualTime,
    window: &ProgramView,
    command: EditCommand,
) {
    wait_until_frozen(running);
    time.elapse(TICK);
    window.edit(command);
    wait_until_stopped_at_freeze(running);
}

/// Whether the program wrote `line`.
fn heard(lines: &Lines, line: &str) -> bool {
    lines.all().iter().any(|written| written == line)
}

/// The id of each of `entries`, in order.
fn ids(entries: &[MenuEntry]) -> Vec<u32> {
    entries.iter().map(|entry| entry.id).collect()
}

/// Runs `examples/menus`, its output into `lines`, and waits for its window.
fn run(
    engine: &WaOS,
    arrivals: &Receiver<ProgramView>,
    lines: &Arc<Lines>,
) -> (RunningProgram, ProgramView) {
    let program = shell::load(engine, waos_test_programs::MENUS);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
    let window = arrivals
        .recv_timeout(WITHIN)
        .expect("the program asks for its window");
    lines.wait_for("a window");
    (running, window)
}

/// The key that types `character` going down, as a US keyboard's.
fn key_down(character: char) -> KeyEvent {
    KeyEvent {
        time: 0,
        kind: KeyKind::Down,
        device: KeyDevice::Keyboard,
        physical: 0x0007_0004 + u64::from(character as u32 - 'a' as u32),
        logical: u64::from(character as u32),
        character: Some(character.to_string()),
        synthesized: false,
    }
}
