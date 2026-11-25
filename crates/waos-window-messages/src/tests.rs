use std::path::PathBuf;
use std::time::Duration;

use waos::bindings::wasdk::platform::menus::{
    CheckState, Image, ItemCheck, MenuEntryKind, MenuItem, Provided, ProvidedItem, Shortcut,
    ShortcutModifiers, Submenu,
};
use waos::bindings::wasdk::platform::system::Brightness;
use waos::bindings::wasdk::platform::views::{
    Cursor, EdgeSwipe, Insets, KeyDevice, KeyKind, NumberFormat, OverlayStyle, PointerButtons,
    PointerChange, PointerDeviceKind, PointerSignalKind, TextAffinity, TextCapitalization,
    TextInputConfiguration, TextInputType, TextRange, TextSelection, ViewFocusDirection,
    ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{
    MacosTitleBar, Point, Size, WindowBackground, WindowLevel,
};

use super::*;

/// `message` written and read back, and written again: equal bytes say every field crossed.
fn crosses<M: Serialize + DeserializeOwned>(message: &M) -> M {
    let bytes = encode(message);
    let read: M = decode(&bytes).expect("the message reads back");
    assert_eq!(encode(&read), bytes, "the message reads back as it was");
    read
}

fn editing_value(text: &str) -> TextEditingValue {
    TextEditingValue {
        text: text.to_owned(),
        selection: Some(TextSelection {
            base_offset: 1,
            extent_offset: 3,
            affinity: TextAffinity::Upstream,
            is_directional: true,
        }),
        composing: Some(TextRange { start: 0, end: 2 }),
    }
}

fn frame() -> Rect {
    Rect {
        origin: Point { x: 10.0, y: 20.0 },
        size: Size {
            width: 640.0,
            height: 480.0,
        },
    }
}

fn metrics() -> Metrics {
    let insets = Insets {
        left: 0.0,
        top: 28.0,
        right: 0.0,
        bottom: 0.0,
    };
    Metrics {
        width: 1280,
        height: 960,
        device_pixel_ratio: 2.0,
        padding: insets,
        view_padding: insets,
        view_insets: insets,
    }
}

fn pointer() -> PointerEvent {
    PointerEvent {
        time: 9,
        change: PointerChange::Down,
        kind: PointerDeviceKind::Mouse,
        signal: PointerSignalKind::None,
        device: 1,
        x: 3.5,
        y: 4.5,
        buttons: PointerButtons::PRIMARY | PointerButtons::FORWARD,
        pressure: 1.0,
        pressure_min: 1.0,
        pressure_max: 1.0,
        distance: 0.0,
        distance_max: 0.0,
        size: 0.0,
        radius_major: 0.0,
        radius_minor: 0.0,
        radius_min: 0.0,
        radius_max: 0.0,
        orientation: 0.0,
        tilt: 0.0,
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
        pan_x: 0.0,
        pan_y: 0.0,
        scale: 1.0,
        rotation: 0.0,
    }
}

#[test]
fn a_window_asked_for_crosses_with_its_configuration_and_where_its_layers_are() {
    let config = WindowConfig {
        title: "Notes".to_owned(),
        size: Some(Size {
            width: 400.0,
            height: 300.0,
        }),
        position: None,
        level: WindowLevel::AlwaysOnTop,
        background: WindowBackground::Glass,
        macos_title_bar: MacosTitleBar::Hidden(52.0),
        ..WindowConfig::default()
    };
    let read = crosses(&ToWindow::OpenWindow {
        window: 3,
        config,
        layers: HostedLayers { context_id: 0x5eed },
    });
    let ToWindow::OpenWindow {
        window,
        config,
        layers,
    } = read
    else {
        panic!("{read:?}");
    };
    assert_eq!(window, 3);
    assert_eq!(layers, HostedLayers { context_id: 0x5eed });
    assert_eq!(config.title, "Notes");
    assert_eq!(config.size.map(|size| size.width), Some(400.0));
    assert!(config.position.is_none());
    assert_eq!(config.level, WindowLevel::AlwaysOnTop);
    assert!(
        matches!(config.macos_title_bar, MacosTitleBar::Hidden(52.0)),
        "with macOS's title bar and its band's height"
    );
}

#[test]
fn the_programs_calls_on_a_view_cross_as_they_are() {
    let configuration = TextInputConfiguration {
        input_type: TextInputType::Number(NumberFormat {
            signed: true,
            decimal: false,
        }),
        input_action: TextInputAction::Search,
        obscure_text: true,
        autocorrect: false,
        enable_suggestions: false,
        text_capitalization: TextCapitalization::Words,
        read_only: false,
    };
    let calls = [
        ViewCall::SetCursor(Cursor::NwseResize),
        ViewCall::RequestFocusChange(ViewFocusState::Focused, ViewFocusDirection::Backward),
        ViewCall::SetHandlesBack(true),
        ViewCall::SystemBars(SystemBarsCall::SetOverlayStyle(OverlayStyle {
            status_bar_icons: Some(Brightness::Dark),
            navigation_bar_icons: None,
            status_bar_contrast_enforced: Some(false),
            navigation_bar_contrast_enforced: None,
        })),
        ViewCall::SystemBars(SystemBarsCall::Hide(EdgeSwipe::Deferred)),
        ViewCall::SystemBars(SystemBarsCall::Show),
        ViewCall::TextInput(TextInputCall::SetClient(configuration)),
        ViewCall::TextInput(TextInputCall::SetEditingState(editing_value("hello"))),
        ViewCall::Window(WindowCall::SetFrame(
            frame(),
            Some(Duration::from_millis(250)),
        )),
        ViewCall::Window(WindowCall::SetTitle("Draft".to_owned())),
        ViewCall::Window(WindowCall::SetMinSize(Some(Size {
            width: 320.0,
            height: 240.0,
        }))),
        ViewCall::Window(WindowCall::SetMaxSize(None)),
        ViewCall::Window(WindowCall::Drag),
        ViewCall::Window(WindowCall::SetMaximized(true)),
        ViewCall::Window(WindowCall::SetMinimized(false)),
        ViewCall::Window(WindowCall::SetFullscreen(true, Some(2))),
        ViewCall::Window(WindowCall::SetFullscreen(false, None)),
    ];
    for call in calls {
        crosses(&ToWindow::View { window: 1, call });
    }
}

#[test]
fn a_hello_crosses_with_what_the_app_was_opened_with() {
    let hello = FromWindow::Hello(Hello {
        program: ProgramId::try_from("dev.example.notes".to_owned()).unwrap(),
        launched_for: LaunchReason::UserOpened,
        opened: OpenRequest {
            files: vec![PathBuf::from("/tmp/a note.md")],
            links: vec!["notes://today".to_owned()],
        },
    });
    let FromWindow::Hello(hello) = crosses(&hello) else {
        panic!("a hello");
    };
    assert_eq!(hello.program.as_str(), "dev.example.notes");
    assert!(matches!(hello.launched_for, LaunchReason::UserOpened));
    assert_eq!(hello.opened.files, [PathBuf::from("/tmp/a note.md")]);
}

#[test]
fn what_a_window_reports_crosses_as_it_is() {
    let reports = [
        ViewReport::Pointer {
            event: pointer(),
            held_keys: vec![(0x0007_00e1, 0x2_0000_0102)],
        },
        ViewReport::TextInputEditingValue(editing_value("draft")),
        ViewReport::Edit(EditCommand::SelectAll),
        ViewReport::PopRoute,
        ViewReport::MetricsChanged(metrics()),
        ViewReport::ScreenChanged(Some(2)),
        ViewReport::PresenceChanged(Presence {
            on_screen: true,
            occluded: false,
            focused: false,
        }),
        ViewReport::WindowFrameChanged(frame()),
        ViewReport::WindowMaximizedChanged(true),
        ViewReport::WindowFullscreenChanged(true),
        ViewReport::TitleBarDoubleClickActionChanged(Some(TitleBarAction::Minimize)),
        ViewReport::TitleBarDoubleClickActionChanged(None),
    ];
    for report in reports {
        crosses(&FromWindow::View { window: 4, report });
    }
    let opened = OpenedWindow {
        metrics: metrics(),
        screen: None,
        presence: Presence {
            on_screen: true,
            occluded: false,
            focused: true,
        },
        frame: frame(),
        size_limits: true,
        maximized: true,
        fullscreen: true,
        title_bar_double_click_action: Some(TitleBarAction::Nothing),
    };
    let FromWindow::WindowOpened { opened, .. } =
        crosses(&FromWindow::WindowOpened { window: 4, opened })
    else {
        panic!("a window opened");
    };
    assert!(opened.size_limits, "with whether it has size limits");
    assert!(opened.maximized, "with whether it is maximized");
    assert!(opened.fullscreen, "and whether it is full screen");
    assert_eq!(
        opened.title_bar_double_click_action,
        Some(TitleBarAction::Nothing),
        "with what a double-click on a title bar does"
    );
}

#[test]
fn a_pointers_buttons_cross_bit_for_bit() {
    let FromWindow::View {
        report: ViewReport::Pointer { event, .. },
        ..
    } = crosses(&FromWindow::View {
        window: 1,
        report: ViewReport::Pointer {
            event: pointer(),
            held_keys: Vec::new(),
        },
    })
    else {
        panic!("a pointer");
    };
    assert_eq!(
        event.buttons,
        PointerButtons::PRIMARY | PointerButtons::FORWARD
    );
}

#[test]
fn a_key_crosses_with_its_id() {
    let key = FromWindow::Key {
        window: 2,
        key: 41,
        event: KeyEvent {
            time: 5,
            kind: KeyKind::Down,
            device: KeyDevice::Keyboard,
            physical: 0x0007_0004,
            logical: 0x61,
            character: Some("a".to_owned()),
            synthesized: false,
        },
    };
    let FromWindow::Key { key, event, .. } = crosses(&key) else {
        panic!("a key");
    };
    assert_eq!(key, 41);
    assert_eq!(event.character.as_deref(), Some("a"));
}

#[test]
fn traced_messages_leave_the_text_out() {
    let report = FromWindow::View {
        window: 1,
        report: ViewReport::TextInputEditingValue(editing_value("hunter2")),
    };
    let key = FromWindow::Key {
        window: 1,
        key: 1,
        event: KeyEvent {
            time: 0,
            kind: KeyKind::Down,
            device: KeyDevice::Keyboard,
            physical: 0,
            logical: 0,
            character: Some("hunter2".to_owned()),
            synthesized: false,
        },
    };
    for traced in [format!("{report:?}"), format!("{key:?}")] {
        assert!(!traced.contains("hunter2"), "{traced}");
    }
}

#[test]
fn a_waos_is_found_by_its_data_folder_alone() {
    let name = bootstrap_name(Path::new("/Users/someone/Library/Application Support/WaOS"));
    assert_eq!(
        name,
        bootstrap_name(Path::new("/Users/someone/Library/Application Support/WaOS"))
    );
    assert_ne!(name, bootstrap_name(Path::new("/tmp/another WaOS")));
    assert!(name.starts_with("dev.waos.app.windows."));
    // The bootstrap server's names are 128 bytes with their NUL.
    assert!(name.len() < 128, "{name}");
}

fn prompt_key(character: &str) -> waft_embedder::KeyData {
    waft_embedder::KeyData {
        time_stamp: Duration::from_millis(12),
        event_type: waft_embedder::KeyEventType::Down,
        device_type: waft_embedder::KeyEventDeviceType::Keyboard,
        physical: 0x0007_0028,
        logical: 0x0010_0000_000d,
        character: Some(character.to_owned()),
        synthesized: false,
    }
}

#[test]
fn a_prompt_asked_for_crosses_with_its_size_and_where_its_layers_are() {
    let asked = crosses(&ToWindow::ShowPrompt {
        prompt: 3,
        size: [440.0, 320.0],
        layers: HostedLayers { context_id: 77 },
    });
    let ToWindow::ShowPrompt {
        prompt,
        size,
        layers,
    } = asked
    else {
        panic!("a prompt");
    };
    assert_eq!((prompt, size, layers.context_id), (3, [440.0, 320.0], 77));
    let call = crosses(&ToWindow::Prompt {
        prompt: 3,
        call: PromptCall::SetCursor(waft_embedder::SystemMouseCursorKind::Click),
    });
    assert!(matches!(
        call,
        ToWindow::Prompt {
            prompt: 3,
            call: PromptCall::SetCursor(waft_embedder::SystemMouseCursorKind::Click),
        }
    ));
}

#[test]
fn what_a_prompts_dialog_reports_crosses_as_waft_reported_it() {
    let pointer = waft_embedder::PointerReport {
        view_id: waft_embedder::ViewId(4),
        time_stamp: Duration::from_micros(1500),
        device: 2,
        kind: waft_embedder::PointerDeviceKind::Trackpad,
        position: [10.5, 20.25],
        change: waft_embedder::ReportedChange::PanZoomUpdate {
            pan: [3.0, -4.0],
            scale: 1.5,
            rotation: 0.25,
        },
        contact: waft_embedder::Contact::default(),
    };
    let read = crosses(&FromWindow::Prompt {
        prompt: 9,
        report: PromptReport::Pointer(pointer),
    });
    let FromWindow::Prompt {
        prompt: 9,
        report: PromptReport::Pointer(read),
    } = read
    else {
        panic!("a dialog's pointer");
    };
    assert_eq!(read, pointer);
    let metrics = waft_embedder::ViewMetrics {
        physical_size: [880.0, 640.0],
        physical_constraints: waft_embedder::ViewConstraints::tight(880.0, 640.0),
        device_pixel_ratio: 2.0,
        screen: Some(waft_embedder::ScreenId(69_733_248)),
        ..waft_embedder::ViewMetrics::default()
    };
    let FromWindow::Prompt {
        report: PromptReport::Shown(shown),
        ..
    } = crosses(&FromWindow::Prompt {
        prompt: 9,
        report: PromptReport::Shown(metrics),
    })
    else {
        panic!("a dialog shown");
    };
    assert_eq!(shown, metrics);
    let FromWindow::Prompt {
        report: PromptReport::Key(key),
        ..
    } = crosses(&FromWindow::Prompt {
        prompt: 9,
        report: PromptReport::Key(prompt_key("\r")),
    })
    else {
        panic!("a dialog's key");
    };
    assert_eq!(key, prompt_key("\r"));
}

#[test]
fn a_traced_prompt_key_leaves_its_character_out() {
    let key = FromWindow::Prompt {
        prompt: 1,
        report: PromptReport::Key(prompt_key("hunter2")),
    };
    let traced = format!("{key:?}");
    assert!(!traced.contains("hunter2"), "{traced}");
}

fn menu_entry(id: u32, parent: Option<u32>, kind: MenuEntryKind) -> MenuEntry {
    MenuEntry { id, parent, kind }
}

/// A bar with an entry of every kind: a submenu shown and one greyed, a provided item, a
/// separator, an item with a mixed checkbox, a shortcut and an icon, and a radio item.
fn menu_bar() -> Vec<MenuEntry> {
    let submenu = |label: &str, enabled| {
        MenuEntryKind::Submenu(Submenu {
            label: label.to_owned(),
            enabled,
        })
    };
    let provided = |item| {
        MenuEntryKind::Provided(Provided {
            item,
            enabled: false,
        })
    };
    let greet = MenuItem {
        label: "Greet".to_owned(),
        enabled: true,
        check: ItemCheck::Checkbox(CheckState::Mixed),
        shortcut: Some(Shortcut {
            logical: u64::from('g'),
            modifiers: ShortcutModifiers::META | ShortcutModifiers::SHIFT,
        }),
        icon: Some(Image {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
            template: true,
        }),
    };
    let shout = MenuItem {
        label: "Shout".to_owned(),
        enabled: false,
        check: ItemCheck::Radio(true),
        shortcut: None,
        icon: None,
    };
    vec![
        menu_entry(1, None, submenu("Notes", true)),
        menu_entry(2, Some(1), provided(ProvidedItem::About)),
        menu_entry(3, Some(1), MenuEntryKind::Separator),
        menu_entry(10, None, submenu("File", false)),
        menu_entry(11, Some(10), MenuEntryKind::Item(greet)),
        menu_entry(12, Some(10), MenuEntryKind::Item(shout)),
        menu_entry(13, Some(10), provided(ProvidedItem::ToggleFullScreen)),
    ]
}

#[test]
fn a_programs_menu_bar_crosses_whole_entry_for_entry() {
    let ToWindow::SetMenuBar(read) = crosses(&ToWindow::SetMenuBar(menu_bar())) else {
        panic!("a menu bar");
    };
    let ids: Vec<(u32, Option<u32>)> = read.iter().map(|entry| (entry.id, entry.parent)).collect();
    assert_eq!(
        ids,
        [
            (1, None),
            (2, Some(1)),
            (3, Some(1)),
            (10, None),
            (11, Some(10)),
            (12, Some(10)),
            (13, Some(10))
        ]
    );
    let MenuEntryKind::Item(greet) = &read[4].kind else {
        panic!("Greet: {:?}", read[4]);
    };
    assert!(matches!(
        greet.check,
        ItemCheck::Checkbox(CheckState::Mixed)
    ));
    let shortcut = greet.shortcut.expect("Greet's shortcut");
    assert_eq!(shortcut.logical, u64::from('g'));
    assert_eq!(
        shortcut.modifiers,
        ShortcutModifiers::META | ShortcutModifiers::SHIFT
    );
    let icon = greet.icon.as_ref().expect("Greet's icon");
    assert_eq!(
        (icon.rgba.as_slice(), icon.template),
        (&[1, 2, 3, 4][..], true)
    );
    assert!(matches!(
        read[6].kind,
        MenuEntryKind::Provided(Provided {
            item: ProvidedItem::ToggleFullScreen,
            enabled: false,
        })
    ));
}

#[test]
fn a_context_menu_and_the_choices_cross_with_the_ids_that_answer_them() {
    let asked = crosses(&ToWindow::ShowContextMenu {
        menu: 7,
        window: 2,
        at: (10.5, 20.25),
        entries: menu_bar(),
    });
    let ToWindow::ShowContextMenu {
        menu,
        window,
        at,
        entries,
    } = asked
    else {
        panic!("a context menu: {asked:?}");
    };
    assert_eq!((menu, window, at, entries.len()), (7, 2, (10.5, 20.25), 7));
    let closed = crosses(&FromWindow::ContextMenuClosed {
        menu: 7,
        chosen: Some(11),
    });
    assert!(
        matches!(
            closed,
            FromWindow::ContextMenuClosed {
                menu: 7,
                chosen: Some(11)
            }
        ),
        "{closed:?}"
    );
    let chosen = crosses(&FromWindow::MenuBarChosen(11));
    assert!(
        matches!(chosen, FromWindow::MenuBarChosen(11)),
        "{chosen:?}"
    );
}

#[test]
fn a_quit_and_the_programs_answer_cross() {
    let asked = crosses(&FromWindow::QuitRequested);
    assert!(matches!(asked, FromWindow::QuitRequested), "{asked:?}");
    for response in [AppExitResponse::Exit, AppExitResponse::Cancel] {
        let answered = crosses(&ToWindow::QuitAnswered(response));
        assert!(
            matches!(answered, ToWindow::QuitAnswered(crossed) if crossed == response),
            "{answered:?}"
        );
    }
}

fn text_type() -> waos::FileType {
    waos::FileType {
        label: "Text".to_owned(),
        extensions: vec!["txt".to_owned()],
        mime_types: vec!["text/plain".to_owned()],
    }
}

#[test]
fn a_file_dialog_of_each_kind_crosses_with_what_it_offers() {
    let dialogs = [
        FileDialog::OpenFile(OpenOptions {
            types: vec![text_type()],
            initial_folder: Some(PathBuf::from("/Users/someone/Notes")),
            confirm_label: Some("Read".to_owned()),
            writable: true,
        }),
        FileDialog::OpenFiles(OpenOptions::default()),
        FileDialog::OpenFolder(FolderOptions {
            initial_folder: None,
            confirm_label: Some("Count".to_owned()),
            writable: true,
        }),
        FileDialog::SaveFile(SaveOptions {
            types: vec![text_type()],
            initial_folder: None,
            suggested_name: Some("saved.txt".to_owned()),
            confirm_label: None,
        }),
    ];
    for kind in dialogs {
        crosses(&ToWindow::OpenFileDialog {
            dialog: 4,
            parent: Some(2),
            kind,
        });
    }
    let asked = crosses(&ToWindow::OpenFileDialog {
        dialog: 5,
        parent: None,
        kind: FileDialog::OpenFile(OpenOptions {
            types: vec![text_type()],
            writable: true,
            ..OpenOptions::default()
        }),
    });
    let ToWindow::OpenFileDialog {
        dialog: 5,
        parent: None,
        kind: FileDialog::OpenFile(options),
    } = asked
    else {
        panic!("a file dialog: {asked:?}");
    };
    assert_eq!(options.types, [text_type()]);
    assert!(options.writable);
}

#[test]
fn what_a_file_dialog_answers_crosses_with_the_id_that_asked_it() {
    let paths = vec![PathBuf::from("/tmp/a note.md"), PathBuf::from("/tmp/b.md")];
    let picked = crosses(&FromWindow::FileDialogClosed {
        dialog: 6,
        picked: Ok(paths.clone()),
    });
    assert!(
        matches!(&picked, FromWindow::FileDialogClosed { dialog: 6, picked: Ok(read) } if *read == paths),
        "{picked:?}"
    );
    let cancelled = crosses(&FromWindow::FileDialogClosed {
        dialog: 7,
        picked: Ok(Vec::new()),
    });
    assert!(
        matches!(&cancelled, FromWindow::FileDialogClosed { dialog: 7, picked: Ok(read) } if read.is_empty()),
        "{cancelled:?}"
    );
    let failed = crosses(&FromWindow::FileDialogClosed {
        dialog: 8,
        picked: Err(DialogError::Failed("no panel".to_owned())),
    });
    assert!(
        matches!(&failed, FromWindow::FileDialogClosed { dialog: 8, picked: Err(DialogError::Failed(why)) } if why == "no panel"),
        "{failed:?}"
    );
}

fn click() -> Activation {
    Activation {
        id: "inbox/7".to_owned(),
        action: Some("answer".to_owned()),
        reply: Some("see you at three".to_owned()),
    }
}

#[test]
fn a_notification_to_post_crosses_whole_with_its_time_and_the_id_that_answers_it() {
    let notification = Notification {
        id: "reminder".to_owned(),
        title: "Remember".to_owned(),
        body: Some("The meeting".to_owned()),
        image: Some(Image {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
            template: false,
        }),
        actions: vec![waos::NotificationAction {
            id: "answer".to_owned(),
            label: "Answer".to_owned(),
            reply_placeholder: Some("Say something".to_owned()),
        }],
        silent: true,
    };
    let at = std::time::UNIX_EPOCH + Duration::from_secs(4_102_444_800);
    let asked = crosses(&ToWindow::PostNotification {
        post: 3,
        notification,
        at: Some(at),
    });
    let ToWindow::PostNotification {
        post: 3,
        notification,
        at: Some(read_at),
    } = asked
    else {
        panic!("a post: {asked:?}");
    };
    assert_eq!(read_at, at);
    assert_eq!(notification.body.as_deref(), Some("The meeting"));
    assert_eq!(
        notification.actions[0].reply_placeholder.as_deref(),
        Some("Say something")
    );
    assert!(matches!(
        crosses(&ToWindow::RemoveNotification {
            id: "old".to_owned()
        }),
        ToWindow::RemoveNotification { id } if id == "old"
    ));
}

#[test]
fn what_a_post_answers_crosses_with_the_id_that_asked_it() {
    for posted in [
        Ok(()),
        Err(NotificationError::Denied),
        Err(NotificationError::Failed("no center".to_owned())),
    ] {
        let read = crosses(&FromWindow::NotificationPosted {
            post: 4,
            posted: posted.clone(),
        });
        let FromWindow::NotificationPosted {
            post: 4,
            posted: read,
        } = read
        else {
            panic!("an answer: {read:?}");
        };
        assert_eq!(format!("{read:?}"), format!("{posted:?}"));
    }
}

#[test]
fn a_click_crosses_while_the_app_runs_and_as_the_reason_it_was_launched() {
    let FromWindow::NotificationActivated(heard) =
        crosses(&FromWindow::NotificationActivated(click()))
    else {
        panic!("a click");
    };
    assert_eq!(heard.reply.as_deref(), Some("see you at three"));
    let hello = FromWindow::Hello(Hello {
        program: ProgramId::try_from("dev.example.notes".to_owned()).unwrap(),
        launched_for: LaunchReason::NotificationActivated(click()),
        opened: OpenRequest::default(),
    });
    let FromWindow::Hello(Hello {
        launched_for: LaunchReason::NotificationActivated(heard),
        ..
    }) = crosses(&hello)
    else {
        panic!("a hello for a click");
    };
    assert_eq!(heard.id, "inbox/7");
    assert_eq!(heard.action.as_deref(), Some("answer"));
}

#[test]
fn a_traced_click_leaves_its_reply_out() {
    let while_running = format!("{:?}", FromWindow::NotificationActivated(click()));
    let launching = format!("{:?}", LaunchReason::NotificationActivated(click()));
    for traced in [while_running, launching] {
        assert!(traced.contains("inbox/7"), "{traced}");
        assert!(!traced.contains("see you at three"), "{traced}");
    }
}

#[test]
fn the_question_of_macos_and_what_the_app_reads_of_its_permission_cross_with_the_id_that_asked() {
    let asked = crosses(&ToWindow::RequestNotificationPermission { request: 6 });
    assert!(matches!(
        asked,
        ToWindow::RequestNotificationPermission { request: 6 }
    ));
    let answered = crosses(&FromWindow::NotificationPermissionAnswered {
        request: 6,
        state: SystemPermissionState::Denied,
    });
    assert!(matches!(
        answered,
        FromWindow::NotificationPermissionAnswered {
            request: 6,
            state: SystemPermissionState::Denied
        }
    ));
    for state in [
        SystemPermissionState::NotAsked,
        SystemPermissionState::Allowed,
        SystemPermissionState::Denied,
    ] {
        let read = crosses(&FromWindow::NotificationPermission { state });
        assert!(
            matches!(read, FromWindow::NotificationPermission { state: read } if read == state)
        );
    }
}

#[test]
fn waos_asking_the_app_to_read_macos_permission_again_crosses() {
    assert!(matches!(
        crosses(&ToWindow::ReadNotificationPermission),
        ToWindow::ReadNotificationPermission
    ));
}

#[test]
fn a_picture_shown_crosses_with_its_window_and_its_size_in_pixels() {
    let shown = crosses(&ToWindow::PictureShown {
        window: 3,
        size: [1600, 1200],
    });
    assert!(
        matches!(
            shown,
            ToWindow::PictureShown {
                window: 3,
                size: [1600, 1200]
            }
        ),
        "{shown:?}"
    );
}

/// The first two messages' bytes in the protocol this crate says, [`PROTOCOL`]: what a WaOS and a
/// window process of different builds first read of each other. A change to either fails this
/// test until `PROTOCOL` is raised and the bytes here are made again.
#[test]
fn the_handshake_is_as_this_protocol_says_it() {
    let hello = FromWindow::Hello(Hello {
        program: ProgramId::try_from("dev.example.notes".to_owned()).unwrap(),
        launched_for: LaunchReason::UserOpened,
        opened: OpenRequest {
            files: Vec::new(),
            links: Vec::new(),
        },
    });
    let welcome = ToWindow::Welcome(Welcome {
        name: "Notes".to_owned(),
    });
    let said = (
        PROTOCOL,
        hex::encode(encode(&hello)),
        hex::encode(encode(&welcome)),
    );
    let pinned = (
        7,
        "00116465762e6578616d706c652e6e6f746573010000".to_owned(),
        "00054e6f746573".to_owned(),
    );
    assert_eq!(
        said, pinned,
        "the handshake changed: raise PROTOCOL, and pin what it says now"
    );
}
