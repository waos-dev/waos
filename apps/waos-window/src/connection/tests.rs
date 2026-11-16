use futures::FutureExt;
use waos::WindowConfig;
use waos::bindings::wasdk::platform::menus::{MenuEntryKind, Submenu};
use waos::bindings::wasdk::platform::views::{
    KeyDevice, KeyKind, ViewFocusDirection, ViewFocusState,
};
use waos_window_messages::decode;
use waos_window_messages::os::macos::{Arrival, ReceiveRight};

use super::*;

/// A connection over a port of the test's own, which plays WaOS's side.
pub(crate) fn connection() -> (Connection, ReceiveRight) {
    let waos = ReceiveRight::new().unwrap();
    let welcome = Welcome {
        name: "Notes".to_owned(),
    };
    let waits = Arc::new(ResizeWaits::default());
    (
        Connection::welcomed(welcome, waos.make_send().unwrap(), waits),
        waos,
    )
}

/// The next message WaOS's side received.
pub(crate) fn sent(waos: &ReceiveRight) -> FromWindow {
    match waos.receive(PROTOCOL).unwrap() {
        Arrival::Message(received) => decode(&received.bytes).unwrap(),
        Arrival::NoSenders => panic!("a message"),
    }
}

fn key() -> KeyEvent {
    KeyEvent {
        time: 0,
        kind: KeyKind::Down,
        device: KeyDevice::Keyboard,
        physical: 0x0007_0004,
        logical: 0x61,
        character: Some("a".to_owned()),
        synthesized: false,
    }
}

#[test]
fn a_key_is_answered_by_the_reply_naming_it() {
    let (connection, waos) = connection();
    let first = connection.key(1, key());
    let second = connection.key(1, key());
    let FromWindow::Key { key: first_id, .. } = sent(&waos) else {
        panic!("a key");
    };
    let FromWindow::Key { key: second_id, .. } = sent(&waos) else {
        panic!("a key");
    };
    assert_ne!(first_id, second_id);
    connection.received(ToWindow::KeyReplied {
        key: second_id,
        handled: true,
    });
    assert_eq!(second.now_or_never(), Some(true));
    assert_eq!(first.now_or_never(), None, "its reply has not come");
}

#[test]
fn a_key_waos_never_answers_was_not_handled_once_the_connection_ends() {
    let (connection, _waos) = connection();
    let waiting = connection.key(1, key());
    connection.end();
    assert_eq!(waiting.now_or_never(), Some(false));
    assert!(connection.ended().now_or_never().is_some());
}

#[test]
fn a_call_goes_to_the_window_it_names() {
    let (connection, _waos) = connection();
    let (calls, mut arriving) = futures::channel::mpsc::unbounded();
    connection.route(2, calls);
    let call = ViewCall::RequestFocusChange(ViewFocusState::Focused, ViewFocusDirection::Undefined);
    assert!(matches!(
        connection.received(ToWindow::View { window: 2, call }),
        Carried::Done
    ));
    assert!(matches!(
        arriving.try_recv(),
        Ok(ViewCall::RequestFocusChange(ViewFocusState::Focused, _))
    ));
    connection.unroute(2);
    let call =
        ViewCall::RequestFocusChange(ViewFocusState::Unfocused, ViewFocusDirection::Undefined);
    connection.received(ToWindow::View { window: 2, call });
    assert!(arriving.try_recv().is_err(), "a window gone hears nothing");
}

#[test]
fn a_window_asked_for_is_the_request_it_answers_its_layers_kept_until_it_opens() {
    let (connection, _waos) = connection();
    let config = WindowConfig {
        title: "Draft".to_owned(),
        ..WindowConfig::default()
    };
    let layers = HostedLayers { context_id: 41 };
    let Carried::WindowAsked(request) = connection.received(ToWindow::OpenWindow {
        window: 5,
        config,
        layers,
    }) else {
        panic!("a request");
    };
    assert_eq!(request.id, 5);
    assert_eq!(request.config.title, "Draft");
    assert_eq!(connection.take_layers(5), Some(layers));
    assert_eq!(
        connection.take_layers(5),
        None,
        "taken once, as its window opens"
    );
}

#[test]
fn a_message_takes_its_ports_to_waos() {
    let (connection, waos) = connection();
    let fence = ReceiveRight::new().unwrap();
    connection.send_with_ports(
        FromWindow::WindowClosed { window: 4 },
        vec![fence.make_send().unwrap()],
    );
    let Arrival::Message(received) = waos.receive(PROTOCOL).unwrap() else {
        panic!("a message");
    };
    assert!(matches!(
        decode(&received.bytes).unwrap(),
        FromWindow::WindowClosed { window: 4 }
    ));
    assert_eq!(received.ports.len(), 1, "the port is WaOS's now");
}

#[test]
fn the_programs_end_ends_the_connection_and_the_window_process() {
    let (connection, _waos) = connection();
    let waiting = connection.key(1, key());
    assert!(matches!(
        connection.received(ToWindow::ProgramEnded),
        Carried::ProgramEnded
    ));
    assert!(connection.ended().now_or_never().is_some());
    assert_eq!(waiting.now_or_never(), Some(false));
}

#[test]
fn a_prompt_and_its_end_are_left_for_the_window_process_to_carry_out() {
    let (connection, _waos) = connection();
    let asked = connection.received(ToWindow::ShowPrompt {
        prompt: 5,
        size: [440.0, 320.0],
        layers: HostedLayers { context_id: 9 },
    });
    assert!(matches!(
        asked,
        Carried::PromptAsked {
            prompt: 5,
            size: [440.0, 320.0],
            layers: HostedLayers { context_id: 9 },
        }
    ));
    assert!(matches!(
        connection.received(ToWindow::HidePrompt { prompt: 5 }),
        Carried::PromptHidden(5)
    ));
}

fn file_menu() -> Vec<MenuEntry> {
    let file = Submenu {
        label: "File".to_owned(),
        enabled: true,
    };
    vec![MenuEntry {
        id: 10,
        parent: None,
        kind: MenuEntryKind::Submenu(file),
    }]
}

#[test]
fn the_programs_menu_bar_and_a_context_menu_are_left_for_the_window_process_to_show() {
    let (connection, _waos) = connection();
    let Carried::MenuBarSet(bar) = connection.received(ToWindow::SetMenuBar(file_menu())) else {
        panic!("a menu bar");
    };
    assert_eq!(bar.iter().map(|entry| entry.id).collect::<Vec<_>>(), [10]);
    let asked = connection.received(ToWindow::ShowContextMenu {
        menu: 3,
        window: 2,
        at: (10.0, 20.0),
        entries: file_menu(),
    });
    let Carried::ContextMenuAsked(asked) = asked else {
        panic!("a context menu");
    };
    assert_eq!(
        (asked.menu, asked.window, asked.at, asked.entries.len()),
        (3, 2, (10.0, 20.0), 1)
    );
}

#[test]
fn a_file_dialog_is_left_for_the_window_process_to_open() {
    let (connection, _waos) = connection();
    let asked = connection.received(ToWindow::OpenFileDialog {
        dialog: 6,
        parent: Some(2),
        kind: waos_window_messages::FileDialog::SaveFile(waos::SaveOptions {
            suggested_name: Some("saved.txt".to_owned()),
            ..waos::SaveOptions::default()
        }),
    });
    let Carried::FileDialogAsked(asked) = asked else {
        panic!("a file dialog");
    };
    assert_eq!((asked.dialog, asked.parent), (6, Some(2)));
    assert!(matches!(
        asked.kind,
        waos_window_messages::FileDialog::SaveFile(options)
            if options.suggested_name.as_deref() == Some("saved.txt")
    ));
}

#[test]
fn a_notification_and_its_removal_are_left_for_the_window_process_to_carry_out() {
    let (connection, _waos) = connection();
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(60);
    let asked = connection.received(ToWindow::PostNotification {
        post: 5,
        notification: waos::Notification {
            id: "reminder".to_owned(),
            title: "Remember".to_owned(),
            body: None,
            image: None,
            actions: Vec::new(),
            silent: false,
        },
        at: Some(at),
    });
    let Carried::NotificationAsked(asked) = asked else {
        panic!("a notification");
    };
    assert_eq!(
        (asked.post, asked.notification.id.as_str(), asked.at),
        (5, "reminder", Some(at))
    );
    let removed = connection.received(ToWindow::RemoveNotification {
        id: "old".to_owned(),
    });
    assert!(matches!(removed, Carried::NotificationRemovalAsked(id) if id == "old"));
}

#[test]
fn waos_asking_for_macos_permission_to_be_read_again_is_left_for_the_window_process() {
    let (connection, _waos) = connection();
    assert!(matches!(
        connection.received(ToWindow::ReadNotificationPermission),
        Carried::NotificationPermissionReadingAsked
    ));
}
