//! An Waft app with menus, over `wasdk-adapter-waft`: a window with a text field holding a line,
//! under a `PlatformMenuBar` of an app menu, File with Greet, and Edit's Copy and Paste, the
//! platform's, and File's Open…, which asks for a text file in the platform's dialog and prints
//! it, Open Link, which opens a web page in the app the system picks, and Notify, which asks its
//! user to allow notifications and posts a greeting with a button; and a `PlatformTrayIcon`, a
//! dot, whose menu has Open, shown on the platform's own trigger. The text field leaves ⌘C, ⌘X
//! and ⌘V to a host with a menu bar, whose Edit commands it acts on. Clicking the icon opens a
//! context menu at the window's corner. It hears what its user opens with it, and what they do to
//! its notifications.
//!
//! It prints `field: focused` once the field has the focus, each change of the field's text as
//! `field: text`, each choice and what the user does to the icon as `what: result`, each file
//! opened with it as `opened with it: path: text` and each link as `opened with it: link`, and
//! each click on its notifications as `notification: id action reply`.
//!
//! ```text
//! cargo build -p waft-menus --target wasm32-wasip3
//! cargo waos run -p waft-menus
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::future::Future;
use std::rc::Rc;

use waft_cupertino::{CupertinoApp, CupertinoPageScaffold, CupertinoTextField};
use waft_embedder::{
    FileType, IconImage, MenuEntry, MenuId, MenuItem, Notification, NotificationAction,
    NotificationActivation, Offset, OpenOptions, OpenRequest, PickedFile, WindowConfig,
};
use waft_foundation::{App, Listenable, Listener};
use waft_widgets::{
    AnyTextEditingController, Center, EdgeInsetsGeometry, Focus, IntoWidget, Padding,
    PlainPlatformMenuItem, PlatformMenu, PlatformMenuBar, PlatformMenuItemRef,
    PlatformProvidedMenuItem, PlatformProvidedMenuItemType, PlatformTrayIcon,
    TextEditingController, TextEditingControllerLeaf, run_app_with,
};

/// The line the field starts with.
const LINE: &str = "a line of waft-menus";

/// The page Open Link opens.
const LINK: &str = "https://example.com/";

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        let text = TextEditingController::new(app)
            .text(app, LINE)
            .as_controller();
        print_changes(app, text);
        app.set_open_requested_handler(Some(Rc::new(print_opened_with_it)));
        app.set_notification_activated_handler(Some(Rc::new(print_click)));
        let window = WindowConfig {
            title: "Waft menus".to_owned(),
            size: [420.0, 160.0],
            ..WindowConfig::default()
        };
        let field = CupertinoTextField::new().controller(text).autofocus(true);
        let field = Focus::new(field).on_focus_change(|_app, focused| {
            if focused {
                println!("field: focused");
            }
        });
        let page = CupertinoPageScaffold::new(
            Center::new().child(Padding::new(EdgeInsetsGeometry::all(24.0)).child(field)),
        );
        let tray_icon = tray_icon().child(CupertinoApp::new().home(page));
        let app_with_menus = PlatformMenuBar::new(menu_bar()).child(tray_icon);
        run_app_with(app, app_with_menus.into_widget(), window);
    })
    .await;
}

/// The app menu, the platform's About and Quit; File, with Greet, Open…, Open Link and Notify;
/// and Edit, the platform's Copy and Paste.
fn menu_bar() -> Vec<PlatformMenuItemRef> {
    let provided = |item| Rc::new(PlatformProvidedMenuItem::new(item)) as PlatformMenuItemRef;
    let greet = PlainPlatformMenuItem::new("Greet")
        .on_selected(Rc::new(|_app: &mut App| println!("menu bar: greet")));
    vec![
        Rc::new(PlatformMenu::new(
            "Waft menus",
            vec![
                provided(PlatformProvidedMenuItemType::About),
                provided(PlatformProvidedMenuItemType::Quit),
            ],
        )),
        Rc::new(PlatformMenu::new(
            "File",
            vec![
                Rc::new(greet),
                Rc::new(PlainPlatformMenuItem::new("Open…").on_selected(Rc::new(open_a_file))),
                Rc::new(PlainPlatformMenuItem::new("Open Link").on_selected(Rc::new(open_a_link))),
                Rc::new(PlainPlatformMenuItem::new("Notify").on_selected(Rc::new(notify))),
            ],
        )),
        Rc::new(PlatformMenu::new(
            "Edit",
            vec![
                provided(PlatformProvidedMenuItemType::Copy),
                provided(PlatformProvidedMenuItemType::Paste),
            ],
        )),
    ]
}

/// Asks for a text file to open, on the window, and prints what it holds.
fn open_a_file(app: &mut App) {
    let platform = app.platform();
    let (Some(dialogs), Some(window)) =
        (platform.file_dialogs(), platform.views().first().cloned())
    else {
        return println!("file dialogs: none");
    };
    let options = OpenOptions {
        types: vec![FileType {
            label: "Text".to_owned(),
            extensions: vec!["txt".to_owned()],
            mime_types: vec!["text/plain".to_owned()],
        }],
        ..OpenOptions::default()
    };
    let picked = dialogs.open_file(Some(window.id()), options);
    app.spawn(async move |_cx| match picked.await {
        Ok(Some(PickedFile::Path(path))) => {
            println!(
                "opened: {}",
                std::fs::read_to_string(path).unwrap_or_default()
            );
        }
        Ok(Some(_)) => println!("opened: no path"),
        Ok(None) => println!("opened: cancelled"),
        Err(error) => println!("opened: {error}"),
    });
}

/// Opens [`LINK`] in the app the system picks, and prints whether it did.
fn open_a_link(app: &mut App) {
    let platform = app.platform();
    let Some(opener) = platform.opener() else {
        return println!("opener: none");
    };
    let opened = opener.open_link(LINK);
    app.spawn(async move |_cx| match opened.await {
        Ok(()) => println!("opened link: ok"),
        Err(error) => println!("opened link: {error}"),
    });
}

/// Asks its user to allow notifications where the platform asks, then posts a greeting with a
/// button, and prints what came of each.
fn notify(app: &mut App) {
    let platform = app.platform();
    let Some(notifications) = platform.notifications() else {
        return println!("notifications: none");
    };
    let asked = notifications
        .permission()
        .map(|permission| permission.request());
    app.spawn(async move |_cx| {
        if let Some(asked) = asked {
            println!("notifications allowed: {:?}", asked.await);
        }
        let Some(notifications) = platform.notifications() else {
            return;
        };
        let mut greeting = Notification::new("greeting", "Hello");
        greeting.body = Some("From waft-menus".to_owned());
        greeting
            .actions
            .push(NotificationAction::button("later", "Later"));
        match notifications.post(&greeting).await {
            Ok(()) => println!("posted: ok"),
            Err(error) => println!("posted: {error}"),
        }
    });
}

/// Prints each file opened with the app, with its text, and each link.
fn print_opened_with_it(_app: &mut App, request: OpenRequest) {
    for file in request.files {
        match file {
            PickedFile::Path(path) => {
                let text = std::fs::read_to_string(&path).unwrap_or_else(|error| error.to_string());
                println!("opened with it: {}: {text}", path.display());
            }
            _ => println!("opened with it: no path"),
        }
    }
    for link in request.links {
        println!("opened with it: {link}");
    }
}

/// Prints a click on one of its notifications.
fn print_click(_app: &mut App, click: NotificationActivation) {
    println!(
        "notification: {} {:?} {:?}",
        click.id, click.action, click.reply
    );
}

/// Prints the field's text at each change.
fn print_changes(app: &mut App, text: AnyTextEditingController) {
    text.add_listener(
        app,
        Listener::new(move |app| println!("field: {}", text.text_value(app))),
    );
}

/// The tray icon, a dot, whose menu has Open, and whose click opens a context menu at the window.
fn tray_icon() -> PlatformTrayIcon {
    let open = PlainPlatformMenuItem::new("Open")
        .on_selected(Rc::new(|_app: &mut App| println!("tray menu: open")));
    PlatformTrayIcon::new(dot())
        .tooltip("Waft menus")
        .menus(vec![Rc::new(open)])
        .on_activated(Rc::new(open_context_menu))
}

/// Opens a context menu at (5, 7) in the window, where the platform has them.
fn open_context_menu(app: &mut App) {
    let platform = app.platform();
    let (Some(menus), Some(window)) = (platform.popup_menus(), platform.views().first().cloned())
    else {
        return;
    };
    let shown = menus.show(window.id(), Offset::new(5.0, 7.0), &[item(40, "Inspect")]);
    say_choice(app, "context menu", shown);
}

/// Says what the user chose in the menu `shown`, once it closes.
fn say_choice(
    app: &mut App,
    menu: &'static str,
    shown: impl Future<Output = Option<MenuId>> + 'static,
) {
    app.spawn(async move |_cx| {
        println!("{menu} chose: {:?}", shown.await);
    });
}

fn item(id: MenuId, label: &str) -> MenuEntry {
    MenuEntry::Item {
        id,
        item: MenuItem::new(label),
    }
}

/// A black dot, drawn as a template in the colour the platform picks.
fn dot() -> IconImage {
    IconImage {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255].into(),
        template: true,
    }
}
