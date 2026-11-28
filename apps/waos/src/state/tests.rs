//! The apps' windows and the system services they reach through Waft, on a desktop of the tests'
//! own over WaOS's one startup kept in memory (`state::test_windows`), each app opened through the
//! app instances from its build.
//!
//! Survey asks for a view, sees it arrive with what it starts with, keeps its window between the
//! size limits it sets, hears a resize, closes its window when asked to and ends once the view is
//! gone, and its state follows its window's focus and whether the window is minimized or occluded;
//! a launch's activation token goes to its next window; triangle draws on its surface, and its
//! window closes when it is stopped; and the clipboard program reads, through Waft's clipboard,
//! what another app copied, puts its own copy there, whose promised forms and file a paste from
//! Waft's clipboard has it produce, hears another app's copy replace it, and ends as its window is
//! asked to close; and the menus program has no menu bar in WaOS's own windows, WaOS's own bar
//! staying while its window is in front, hears Edit's commands through its canvas, opens a context
//! menu through Waft's at a key, and adds a tray icon, which offers the menus Waft's icon offers,
//! hears what Waft reports of it, opens a menu at it, and goes with the program, or fails with why
//! Waft's tray refused it; a key it leaves is offered to the menu bar, and one it handles is not;
//! and the file dialogs program's picks are mounted.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::executor::block_on;
use waft::ui::{
    Clipboard as _, ClipboardEvent, CopyRequest, EditCommand, EmbedderClient, KeyData,
    KeyEventType, MenuEntry, Offset, TrayIconEvent, URI_LIST, View, ViewFocusDirection,
    ViewFocusEvent, ViewFocusState,
};
use waft_test::{AskedDialog, MockTrayIcons};
use waos::{AppInstance, ProgramEnd};

use crate::state::test_system::Lines;
use crate::state::test_windows::Desktop;

fn has_ended(program: &AppInstance) -> Option<ProgramEnd> {
    program.ended().now_or_never()
}

#[test]
fn a_requested_view_arrives_in_its_window_hears_a_resize_and_goes_with_its_window() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    desktop.pump_until("survey's view arriving", |_| {
        output.has(
            "view 1: 900x600 at 1x, on screen Some(1), in a window: true, for request 1 \
             with Some(\"opened at start\")",
        )
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    assert_eq!(window.config().title, "Survey");
    desktop.pump_until("survey's size limits reaching its window", |_| {
        window.max_size().is_some()
    });
    assert_eq!(window.min_size(), Some(waft::Size::new(320.0, 240.0)));
    assert_eq!(window.max_size(), Some(waft::Size::new(1920.0, 1200.0)));

    let view = window.test_view().id();
    window
        .test_view()
        .set_physical_size(waft::ui::Size::new(400.0, 300.0));
    desktop.shell.view_metrics_changed(view);
    desktop.pump_until("survey hearing the resize", |_| {
        output.has("view 1: now 400x300")
    });

    desktop.shell.view_close_requested(view);
    desktop.pump_until("survey closing its window", |_| window.closes() == 1);
    desktop.pump_until("survey's end heard", |_| output.ended().is_some());
    let end = has_ended(&survey).unwrap();
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert!(output.has("view 1: asked to close"), "{:#?}", output.all());
    assert!(output.has("view 1 removed"), "{:#?}", output.all());
    assert_eq!(desktop.windows_made(), 1);
}

#[test]
fn a_launch_for_a_program_with_no_window_yet_gives_its_next_window_the_launchs_token() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    let token = "gnome-shell/Survey/2313-3_TIME121955272";
    let windows = desktop.view_windows.clone();
    windows.update(&mut desktop.shell.app(), |windows, cx| {
        windows.bring_forward_for_launch(survey.name(), Some(token.to_owned()), cx);
    });
    desktop.pump_until("survey's window", |desktop| desktop.windows_made() == 1);
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    assert_eq!(
        window.config().linux.activation_token.as_deref(),
        Some(token)
    );
    assert_eq!(
        window.config().linux.app_id,
        None,
        "a program never installed has no identity"
    );
}

#[test]
fn a_views_presence_follows_its_windows_focus_and_whether_it_is_minimized_or_occluded() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    desktop.pump_until("survey's view arriving", |_| {
        output
            .all()
            .iter()
            .any(|line| line.starts_with("view 1: 900x600"))
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    let view = window.test_view().id();
    let focus = |state| ViewFocusEvent {
        view_id: view,
        state,
        direction: ViewFocusDirection::Undefined,
    };
    let steps = [
        (
            Step::Focus(ViewFocusState::Unfocused),
            "program: LifecycleState::Inactive",
        ),
        (Step::Occluded(true), "program: LifecycleState::Hidden"),
        (Step::Occluded(false), "program: LifecycleState::Inactive"),
        (Step::Minimized(true), "program: LifecycleState::Hidden"),
        (Step::Minimized(false), "program: LifecycleState::Inactive"),
        (
            Step::Focus(ViewFocusState::Focused),
            "program: LifecycleState::Resumed",
        ),
    ];
    for (step, heard) in steps {
        let before = output.all().len();
        match step {
            Step::Focus(state) => desktop.shell.view_focus_changed(focus(state)),
            Step::Occluded(occluded) => {
                window.set_occluded(occluded);
                desktop.shell.window_changed(view);
            }
            Step::Minimized(minimized) => {
                window.set_minimized(minimized);
                desktop.shell.window_changed(view);
            }
        }
        desktop.pump_until(heard, |_| {
            output.all()[before..].iter().any(|line| line == heard)
        });
    }
    survey.stop();
}

/// What the host reports in the presence test: the window's focus, and the window changing as it
/// is covered or uncovered, minimized or restored.
enum Step {
    Focus(ViewFocusState),
    Occluded(bool),
    Minimized(bool),
}

/// Drawing makes hardware buffers, which only Apple's systems have so far.
#[cfg(target_vendor = "apple")]
#[test]
fn a_program_draws_on_its_surface_and_its_window_closes_when_it_ends() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let triangle = desktop.run(waos_test_programs::TRIANGLE, &output);
    desktop.pump_until("triangle presenting ten frames", |desktop| {
        desktop.platform.compositor.presents() >= 10
    });
    triangle.stop();
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    desktop.pump_until("triangle's window closing", |_| window.closes() == 1);
    assert!(matches!(has_ended(&triangle), Some(ProgramEnd::Stopped)));
}

#[test]
fn a_program_reads_another_apps_copy_and_its_own_copy_is_pasted_from_waft_until_replaced() {
    let mut desktop = Desktop::new();
    std::fs::create_dir_all(desktop.folder.path().join("pasted"))
        .expect("the pasted files' folder");
    let output = Arc::new(Lines::default());
    let program = desktop.run_as(
        waos_test_programs::CLIPBOARD,
        "dev.waos.clipboard",
        "Clipboard",
        &output,
    );
    desktop.pump_until("its window", |_| output.has("a window"));
    desktop.focus_first_window();
    desktop.copied_elsewhere("copied elsewhere");
    desktop.pump_until("the other app's copy read", |_| {
        output.has("read: copied elsewhere")
    });
    desktop.pump_until("its own copy", |_| output.has("written"));

    let clipboard = desktop.clipboard.clone();
    assert_eq!(clipboard.clipboard_data().as_deref(), Some("copied text"));
    let options = clipboard.options();
    assert!(options.sensitive && !options.this_device_only);
    assert_eq!(options.expires_after, Some(Duration::from_secs(60)));
    let items = block_on(clipboard.items());
    assert_eq!(items[0].types(), ["text/plain", "text/html", "text/rtf"]);
    let html = block_on(items[0].read("text/html")).expect("its HTML, produced");
    assert_eq!(html, b"<b>copied text</b>");
    assert_eq!(items[1].types(), [URI_LIST]);
    let files = block_on(items[1].read(URI_LIST)).expect("its files");
    let files = String::from_utf8(files).expect("a list of URLs");
    let picture = desktop.folder.path().join("pasted").join("picture.png");
    assert_eq!(
        std::fs::read(&picture).expect("the picture, written"),
        b"PNG"
    );
    let names: Vec<&str> = files
        .lines()
        .map(|url| url.rsplit('/').next().unwrap_or_default())
        .collect();
    assert_eq!(names, ["note.txt", "picture.png"]);

    desktop.copied_elsewhere("another copy");
    desktop.pump_until("its copy replaced", |_| output.has("replaced"));
    let window = desktop.platform.windows.made()[0].test_view().id();
    desktop.shell.view_close_requested(window);
    desktop.pump_until("its end heard", |_| output.ended().is_some());
    assert!(output.has("flushed"));
    assert!(matches!(
        has_ended(&program),
        Some(ProgramEnd::Returned(Ok(())))
    ));
}

#[test]
fn waos_own_bar_stays_while_a_programs_window_is_in_front_and_edits_commands_reach_the_program() {
    let mut desktop = Desktop::new();
    let bar = desktop.platform.menu_bar.clone();
    assert_eq!(
        titles(&bar.menus()),
        ["WaOS", "Edit", "Window"],
        "WaOS's own"
    );
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its window", |_| output.has("menu bar: none"));
    desktop.focus_first_window();
    desktop.pump();

    let view = desktop.platform.windows.made()[0].test_view().id();
    let request = CopyRequest::new(|_items, _options| Box::pin(std::future::ready(Ok(()))));
    desktop
        .shell
        .clipboard_event(view, ClipboardEvent::Copy(request));
    desktop.pump_until("the user's copy", |_| output.has("copy: written"));
    desktop
        .shell
        .clipboard_event(view, ClipboardEvent::Paste(Vec::new()));
    desktop.pump_until("the user's paste", |_| output.has("paste: copied by menus"));
    desktop
        .clipboard
        .set_clipboard_data(Some("copied unheard".to_owned()));
    desktop
        .shell
        .clipboard_event(view, ClipboardEvent::Paste(Vec::new()));
    desktop.pump_until("a paste of a copy Waft told nothing of", |_| {
        output.has("paste: copied unheard")
    });
    desktop.shell.edit_command(view, EditCommand::SelectAll);
    desktop.pump_until("select all", |_| output.has("edit: EditCommand::SelectAll"));

    assert_eq!(
        (titles(&bar.menus()), bar.sets()),
        (
            vec!["WaOS".to_owned(), "Edit".to_owned(), "Window".to_owned()],
            1
        ),
        "set once, as WaOS starts: a program's bar is its app's"
    );
    program.stop();
}

#[test]
fn a_programs_context_menu_and_tray_icon_go_through_wafts() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its tray icon", |_| output.has("tray icon: added"));
    desktop.focus_first_window();
    desktop.pump();

    desktop.shell.key_data(key_down('m'));
    desktop.pump_until("the context menu's choice", |_| {
        output.has("context menu chose: Some(40)")
    });
    let shown = desktop.platform.popup_menus.shown();
    let view = desktop.platform.windows.made()[0].test_view().id();
    assert_eq!(
        (shown[0].view, shown[0].at),
        (view, Offset::new(10.0, 20.0))
    );

    let tray = desktop.platform.tray.clone();
    desktop.pump_until("the icon's attached menu", |_| {
        tray.icons()
            .first()
            .is_some_and(|icon| !icon.attached_menu.is_empty())
    });
    let icon = tray.icons()[0].clone();
    assert_eq!(icon.tooltip.as_deref(), Some("Menus"));
    let at = Offset::new(1.0, 2.0);
    desktop
        .shell
        .tray_icon_event(icon.id, TrayIconEvent::Activated { position: at });
    desktop.pump_until("the click", |_| output.has("tray: activated at 1,2"));
    desktop
        .shell
        .tray_icon_event(icon.id, TrayIconEvent::MenuSelected(30));
    desktop.pump_until("the attached menu's choice", |_| {
        output.has("attached menu chose: 30")
    });
    desktop.shell.tray_icon_event(
        icon.id,
        TrayIconEvent::ContextMenuRequested { position: at },
    );
    desktop.pump_until("the menu at the icon", |_| {
        output.has("menu at icon chose: Some(40)")
    });

    program.stop();
    desktop.pump_until("the icon removed", |_| tray.icons().is_empty());
}

#[test]
fn a_key_the_program_leaves_is_offered_to_the_menu_bar_and_one_it_handles_is_not() {
    let mut desktop = Desktop::new();
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its window", |_| output.has("menu bar: none"));
    desktop.focus_first_window();
    desktop.pump();

    for key in ['a', 'b'] {
        desktop.shell.key_data(key_down(key));
    }
    let bar = desktop.platform.menu_bar.clone();
    let offered = || -> Vec<u64> { bar.offered().iter().map(|key| key.logical).collect() };
    desktop.pump_until("A handled, and B left and offered", |_| {
        output.has("key a: handled")
            && output.has("key b: left")
            && offered().contains(&u64::from('b'))
    });
    assert_eq!(offered(), [u64::from('b')], "A, handled, is not offered");
    program.stop();
}

#[test]
fn a_programs_tray_icon_offers_the_menus_wafts_icon_offers() {
    let mut desktop = Desktop::with_tray(MockTrayIcons::without_attached_menus);
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its tray icon", |_| output.has("tray icon: added"));
    assert!(output.has("tray icon menus: attached no, at icon yes"));
    let tray = desktop.platform.tray.clone();
    desktop.pump();
    assert_eq!(tray.icons()[0].attached_menu, [], "no attached menu set");
    program.stop();
}

#[test]
fn a_tray_icon_wafts_tray_refuses_fails_with_why() {
    let mut desktop = Desktop::with_tray(|tray| tray.refusing("no tray here"));
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its tray icon refused", |_| {
        output.has("tray icon: failed (no tray here)")
    });
    assert!(desktop.platform.tray.icons().is_empty());
    program.stop();
}

#[test]
fn a_programs_file_dialogs_go_through_wafts_and_their_picks_are_mounted() {
    let mut desktop = Desktop::new();
    let picks = std::fs::canonicalize(&desktop.folder)
        .unwrap()
        .join("picks");
    std::fs::create_dir_all(picks.join("pictures")).unwrap();
    std::fs::write(picks.join("read-me.txt"), "a line to read").unwrap();
    std::fs::write(picks.join("change-me.txt"), "a line").unwrap();
    for pick in ["read-me.txt", "change-me.txt", "saved.txt", "pictures"] {
        let answers = &desktop.platform.dialog_answers;
        answers.borrow_mut().push_back(picks.join(pick));
    }
    let output = Arc::new(Lines::default());
    let id = "dev.waos.filedialogs";
    desktop.run_as(
        waos_test_programs::FILE_DIALOGS,
        id,
        "File dialogs",
        &output,
    );
    desktop.pump_until("its end heard", |_| output.ended().is_some());
    for line in [
        "read file written to: no",
        "changed: yes",
        "saved safely: yes",
    ] {
        assert!(output.has(line), "no {line:?} in {:#?}", output.all());
    }
    assert_eq!(
        std::fs::read_to_string(picks.join("saved.txt")).unwrap(),
        "second save"
    );
    let asked = desktop.platform.file_dialogs.asked();
    assert!(matches!(
        &asked[2],
        AskedDialog::SaveFile(None, options) if options.suggested_name.as_deref() == Some("saved.txt")
    ));
    let kept = desktop.permissions.of(&id.parse().unwrap()).file_system();
    assert!(
        kept.mounts
            .iter()
            .any(|mount| mount.host == picks.join("saved.txt") && mount.writable),
        "a pick is kept among the program's mounts: {kept:?}"
    );
}

/// The title of each top-level menu of `menus`.
fn titles(menus: &[MenuEntry]) -> Vec<String> {
    menus
        .iter()
        .filter_map(|menu| match menu {
            MenuEntry::Submenu { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

/// The key that types `character` going down, as a US keyboard's.
fn key_down(character: char) -> KeyData {
    KeyData {
        event_type: KeyEventType::Down,
        physical: 0x0007_0004 + u64::from(character as u32 - 'a' as u32),
        logical: u64::from(character),
        character: Some(character.to_string()),
        ..KeyData::default()
    }
}
