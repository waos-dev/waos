//! A program that answers its user's keys, Edit's commands and its menus: it asks for a window,
//! sets its menu bar, an app menu, File and Edit, and adds a tray icon, a dot, saying which menus
//! the icon offers, or why the platform did not show it, and sets its attached menu where it has
//! one; it says which item the user chooses in either, and what the user does to the icon, opening
//! a menu at the icon when the user asks for one there, and says a click again from a second stream
//! of the icon's events. It answers each key that goes down in its window, handling the key at `A`,
//! leaving the key at `B` by dropping its reply unanswered, opening a context menu at the key at
//! `M`, and answering that it did not handle any other. A copy writes a line to the clipboard
//! through the user's request, and tries a second write with it, which is refused; a cut writes
//! another; a paste reads each item pasted, its text, or each file it names, which it reads and
//! tries to write to; and an undo, a redo, a select-all or a delete is said. Asking to close its
//! window ends it, its tray icon going with it.
//!
//! It prints each step as `what: result`.
//!
//! ```text
//! cargo build -p menus --target wasm32-wasip2
//! ```

use wasdk::clipboard::{ClipboardError, Content, CopyOptions, CopyRequest, Paste};
use wasdk::menus::{
    self, CheckState, ContextMenus, Image, ItemCheck, MenuAtIcon, MenuEntry, MenuEntryKind,
    MenuItem, Provided, ProvidedItem, Shortcut, ShortcutModifiers, Submenu, TrayIcon,
    TrayIconError, TrayIconEvent,
};
use wasdk::views::{self, KeyInput, KeyKind, View, ViewChange, ViewEvent};
use wasdk::windows::WindowConfig;
use wasdk::{StreamReader, wit_stream};

#[wasdk::main]
async fn main() {
    let Some(window) = open_window().await else {
        return;
    };
    set_menu_bar();
    let _icon = add_tray_icon().await;
    let context_menus = menus::get_context_menus();
    if context_menus.is_none() {
        println!("context menus: none");
    }
    let Some(mut events) = window.events() else {
        return;
    };
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Key(key) if goes_down_at(&key, 'm') => {
                key.reply.handled(true);
                open_context_menu(context_menus.as_ref(), &window).await;
            }
            ViewEvent::Key(key) => answer(key),
            ViewEvent::Copy(request) => copy(&request, "copy", "copied by menus").await,
            ViewEvent::Cut(request) => copy(&request, "cut", "cut by menus").await,
            ViewEvent::Paste(paste) => read(&paste).await,
            ViewEvent::Edit(command) => println!("edit: {command:?}"),
            ViewEvent::CloseRequested => break,
            _ => {}
        }
    }
    if let Some(frame) = window.window() {
        frame.close();
    }
}

/// Asks for a window and waits for it.
async fn open_window() -> Option<View> {
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Menus");
    if let Err(error) = views::request_view(&config, None) {
        println!("no window: {error}");
        return None;
    }
    while let Some(change) = changes.next().await {
        if let ViewChange::Added(view) = change {
            println!("a window");
            return Some(view);
        }
    }
    None
}

/// Sets the menu bar, and says each item the user chooses in it.
fn set_menu_bar() {
    let Some(bar) = menus::get_menu_bar() else {
        println!("menu bar: none");
        return;
    };
    bar.set(&menu_bar());
    let mut chosen = bar.chosen();
    wasdk::spawn(async move {
        while let Some(id) = chosen.next().await {
            println!("menu bar chose: {id}");
        }
    });
    println!("menu bar: set");
}

/// The app menu, the platform's About and Quit; File, with Greet under ⌘G and Shout, a checkbox
/// off; and Edit, the platform's Cut, Copy and Paste.
fn menu_bar() -> Vec<MenuEntry> {
    vec![
        submenu(1, "Menus"),
        provided(2, 1, ProvidedItem::About),
        separator(3, 1),
        provided(4, 1, ProvidedItem::Quit),
        submenu(10, "File"),
        entry(
            11,
            Some(10),
            MenuEntryKind::Item(MenuItem {
                shortcut: Some(Shortcut {
                    logical: u64::from('g'),
                    modifiers: ShortcutModifiers::META,
                }),
                ..item("Greet")
            }),
        ),
        entry(
            12,
            Some(10),
            MenuEntryKind::Item(MenuItem {
                check: ItemCheck::Checkbox(CheckState::Off),
                ..item("Shout")
            }),
        ),
        submenu(20, "Edit"),
        provided(21, 20, ProvidedItem::Cut),
        provided(22, 20, ProvidedItem::Copy),
        provided(23, 20, ProvidedItem::Paste),
    ]
}

/// Adds the tray icon and sets its attached menu, where the platform has them; says what the
/// user chooses in that menu, and what the user does to the icon, opening a menu at the icon when
/// the user asks for one there.
async fn add_tray_icon() -> Option<TrayIcon> {
    let Some(icons) = menus::get_tray_icons() else {
        println!("tray icons: none");
        return None;
    };
    let icon = match icons.add(dot(), Some("Menus".to_owned())).await {
        Ok(icon) => icon,
        Err(TrayIconError::Failed(reason)) => {
            println!("tray icon: failed ({reason})");
            return None;
        }
    };
    let attached_menu = icon.attached_menu();
    let at_icon = icon.menu_at_icon();
    println!(
        "tray icon menus: attached {}, at icon {}",
        yes_or_no(attached_menu.is_some()),
        yes_or_no(at_icon.is_some())
    );
    if let Some(menu) = attached_menu {
        menu.set(&icon_menu());
        let mut chosen = menu.chosen();
        wasdk::spawn(async move {
            while let Some(id) = chosen.next().await {
                println!("attached menu chose: {id}");
            }
        });
    }
    let mut events = icon.events();
    wasdk::spawn(async move {
        while let Some(event) = events.next().await {
            answer_tray_icon(event, at_icon.as_ref()).await;
        }
    });
    // A second stream of the icon's events, which hears each as the first does.
    let mut events_again = icon.events();
    wasdk::spawn(async move {
        while let Some(event) = events_again.next().await {
            if let TrayIconEvent::Activated(point) = event {
                println!("tray, heard again: activated at {},{}", point.x, point.y);
            }
        }
    });
    println!("tray icon: added");
    Some(icon)
}

/// Says what the user did to the tray icon, and opens a menu at it when the user asks for one.
async fn answer_tray_icon(event: TrayIconEvent, at_icon: Option<&MenuAtIcon>) {
    match event {
        TrayIconEvent::Activated(point) => println!("tray: activated at {},{}", point.x, point.y),
        TrayIconEvent::SecondaryActivated(point) => {
            println!("tray: secondary activated at {},{}", point.x, point.y);
        }
        TrayIconEvent::DoubleClicked => println!("tray: double clicked"),
        TrayIconEvent::Scrolled(delta) => println!("tray: scrolled {delta}"),
        TrayIconEvent::ContextMenuRequested(_) => {
            println!("tray: menu asked for");
            if let Some(menu) = at_icon {
                let chosen = menu.show(icon_menu()).await;
                println!("menu at icon chose: {chosen:?}");
            }
        }
    }
}

/// The tray icon's menu: Open and Quit.
fn icon_menu() -> Vec<MenuEntry> {
    vec![
        entry(30, None, MenuEntryKind::Item(item("Open"))),
        provided_at_top(31, ProvidedItem::Quit),
    ]
}

/// Opens a context menu at the top-left of `window`, where the platform has them, and says what
/// the user chose in it.
async fn open_context_menu(menus: Option<&ContextMenus>, window: &View) {
    let Some(menus) = menus else {
        return;
    };
    let entries = vec![
        entry(40, None, MenuEntryKind::Item(item("Inspect"))),
        provided_at_top(41, ProvidedItem::Copy),
    ];
    let chosen = menus.show(window, 10.0, 20.0, entries).await;
    println!("context menu chose: {chosen:?}");
}

fn yes_or_no(yes: bool) -> &'static str {
    if yes { "yes" } else { "no" }
}

/// A black dot, drawn as a template in the colour the platform picks.
fn dot() -> Image {
    Image {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255],
        template: true,
    }
}

fn entry(id: u32, parent: Option<u32>, kind: MenuEntryKind) -> MenuEntry {
    MenuEntry { id, parent, kind }
}

fn submenu(id: u32, label: &str) -> MenuEntry {
    let submenu = Submenu {
        label: label.to_owned(),
        enabled: true,
    };
    entry(id, None, MenuEntryKind::Submenu(submenu))
}

fn provided(id: u32, parent: u32, item: ProvidedItem) -> MenuEntry {
    entry(id, Some(parent), enabled_provided(item))
}

fn provided_at_top(id: u32, item: ProvidedItem) -> MenuEntry {
    entry(id, None, enabled_provided(item))
}

/// `item`, which the user can choose.
fn enabled_provided(item: ProvidedItem) -> MenuEntryKind {
    MenuEntryKind::Provided(Provided {
        item,
        enabled: true,
    })
}

fn separator(id: u32, parent: u32) -> MenuEntry {
    entry(id, Some(parent), MenuEntryKind::Separator)
}

/// An item of the program's own, enabled, with no check, shortcut or icon.
fn item(label: &str) -> MenuItem {
    MenuItem {
        label: label.to_owned(),
        enabled: true,
        check: ItemCheck::Plain,
        shortcut: None,
        icon: None,
    }
}

/// Whether `key` is the key typing `character` going down.
fn goes_down_at(key: &KeyInput, character: char) -> bool {
    key.event.kind == KeyKind::Down && key.event.logical == u64::from(character)
}

/// Handles the key at `A` going down, leaves the key at `B` by dropping its reply, and says it
/// did not handle any other.
fn answer(key: KeyInput) {
    let KeyInput { event, reply } = key;
    if event.kind != KeyKind::Down {
        reply.handled(false);
        return;
    }
    match char::from_u32(event.logical as u32) {
        Some('a') => {
            reply.handled(true);
            println!("key a: handled");
        }
        Some('b') => {
            drop(reply);
            println!("key b: left");
        }
        _ => {
            reply.handled(false);
            println!("key {:#x}: not handled", event.logical);
        }
    }
}

/// Writes `line` with the user's `request`, then tries again, which a request refuses.
async fn copy(request: &CopyRequest, what: &str, line: &str) {
    for attempt in [what.to_owned(), format!("{what} again")] {
        let items = vec![vec![(
            "text/plain".to_owned(),
            Content::Data(stream_of(line.as_bytes().to_vec())),
        )]];
        match request.write(items, CopyOptions::new()).await {
            Ok(_source) => println!("{attempt}: written"),
            Err(error) => println!("{attempt}: {}", case(&error)),
        }
    }
}

/// Reads each item the user pasted: each file its `text/uri-list` names, or else its text.
async fn read(paste: &Paste) {
    for item in paste.items() {
        let types = item.types();
        let mime = if types.iter().any(|mime| mime == "text/uri-list") {
            "text/uri-list"
        } else {
            "text/plain"
        };
        let content = match item.read(mime.to_owned()).await {
            Ok(content) => String::from_utf8_lossy(&content.collect().await).into_owned(),
            Err(error) => {
                println!("paste: {}", case(&error));
                continue;
            }
        };
        if mime == "text/plain" {
            println!("paste: {content}");
            continue;
        }
        for path in content.lines().filter_map(path_of) {
            open_pasted(&path);
        }
    }
}

/// Reads the file the user pasted at `path`, and tries to write to it.
fn open_pasted(path: &str) {
    match std::fs::read_to_string(path) {
        Ok(text) => println!("pasted file: {text}"),
        Err(error) => println!("pasted file: {error}"),
    }
    let written = std::fs::OpenOptions::new().append(true).open(path);
    println!(
        "pasted file written to: {}",
        if written.is_ok() { "yes" } else { "no" }
    );
}

/// The path of a `file:` URL, its escapes undone; none for another line.
fn path_of(line: &str) -> Option<String> {
    let escaped = line.trim().strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(escaped.len());
    let mut rest = escaped.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let hex = after.get(..2).and_then(|hex| std::str::from_utf8(hex).ok());
        match (byte, hex.and_then(|hex| u8::from_str_radix(hex, 16).ok())) {
            (b'%', Some(decoded)) => {
                bytes.push(decoded);
                rest = &after[2..];
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    String::from_utf8(bytes).ok()
}

/// `bytes`, as a stream the program gives.
fn stream_of(bytes: Vec<u8>) -> StreamReader<u8> {
    let (mut writer, reader) = wit_stream::new::<u8>();
    wasdk::spawn(async move {
        writer.write_all(bytes).await;
    });
    reader
}

/// The error's case, as the WIT names it.
fn case(error: &ClipboardError) -> String {
    match error {
        ClipboardError::Changed => "changed".to_owned(),
        ClipboardError::NotFound => "not-found".to_owned(),
        ClipboardError::Denied => "denied".to_owned(),
        ClipboardError::Unsupported => "unsupported".to_owned(),
        ClipboardError::Failed(reason) => format!("failed ({reason})"),
    }
}
