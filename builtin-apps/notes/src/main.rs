//! Notes: a library of notes the app keeps itself, as Apple's Notes does, and a text editor of
//! files from anywhere beside it, as an Waft app in WinUI's Fluent look (`waft-winui`) over
//! `wasdk-adapter-waft`. What a person uses to try what WaOS gives a program: its own `/data`, a
//! window with size limits and a title, a menu bar with the platform's Edit and Window items, the
//! system's file dialogs and the files WaOS mounts for them, what the system opens it with, its
//! input methods, a notification set for later, and the lifecycle's request to end and its resume.
//!
//! - **The library** keeps every note in the program's `/data`, a file to a note ([`store`]),
//!   each change kept as it is typed ([`library`], [`editing`]); a deleted note stays in Recently
//!   Deleted for thirty days.
//! - **Text files** from anywhere, picked in the open dialog or opened with Notes, are edited in
//!   place and saved as they are typed, never over another app's change ([`opening`],
//!   [`open_files`], [`open_file`], [`outside_changes`]); they are open again at the next launch
//!   ([`open_file_list`]).
//! - **The window** lists the notes and the files in a Fluent navigation view beside the one shown,
//!   or, at a phone's width, in a pane over it ([`main_page`], [`note_list`], [`editor_pane`],
//!   [`list_pane`]); its search narrows the list ([`search`], [`browsing`], [`showing`]), and info
//!   bars say what went wrong ([`notices`]).
//! - **File** makes, opens, closes, imports, exports, deletes and recovers ([`importing`],
//!   [`exporting`]), and sets a reminder ([`reminders`]); Edit and Window are the platform's
//!   ([`menus`]).
//! - **Quitting** asks only where a note or a file could not be saved ([`quitting`]).
//!
//! ```text
//! cargo waos run -p notes
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod bar_button;
mod browsing;
mod dates;
mod editing;
mod editor_pane;
mod exporting;
mod icons;
mod importing;
mod library;
mod list_pane;
mod main_page;
mod menus;
mod note;
mod note_list;
mod notes_app;
mod notices;
mod open_file;
mod open_file_list;
mod open_files;
mod opening;
mod outside_changes;
mod question;
mod quitting;
mod reminders;
mod safe_write;
mod search;
mod secondary_text;
mod showing;
mod store;
mod text_file;
mod window;

use std::rc::Rc;

use waft_embedder::NotificationActivation;
use waft_foundation::App;
use waft_widgets::{
    IntoWidget, PageRouteBuilder, PlatformMenuBar, RouteSettingsRef, WidgetRef, WidgetsApp,
    run_app_with,
};
use waft_winui::{AccentPalette, install_icon_font};

use crate::editing::keep_edits;
use crate::main_page::MainPage;
use crate::menus::menu_bar;
use crate::notes_app::NotesApp;
use crate::opening::{hear_open_requests, say_not_reopened};
use crate::outside_changes::look_again_on_resume;
use crate::quitting::answer_quits;
use crate::reminders::reminder_clicked;
use crate::showing::{follow_search, show_a_listed_item};
use crate::window::{APP_NAME, NotesWindow};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the library the program's `/data` keeps and the files open at its last run, its
/// most recent note shown, or else a file; the edits and the search followed; quits answered,
/// reminders' clicks and the files the system opens it with heard, and the files looked at again
/// at each resume; and the window, under the menu bar.
fn start(app: &mut App) {
    let (notes, unreadable) = NotesApp::open(app);
    keep_edits(app, &notes);
    follow_search(app, &notes);
    show_a_listed_item(app, &notes);
    say_not_reopened(app, &notes, &unreadable);
    answer_quits(app, &notes);
    hear_reminder_clicks(app, &notes);
    hear_open_requests(app, &notes);
    look_again_on_resume(app, &notes);
    let window = NotesWindow {
        notes: notes.clone(),
        child: widgets_app(&notes),
    };
    let content = PlatformMenuBar::new(menu_bar(&notes)).child(window);
    run_app_with(app, content.into_widget(), window::config());
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// The window's app: its navigator, which the dialogs are shown over and the popups in, with the
/// page as its one route, in Fluent's accent.
fn widgets_app(notes: &NotesApp) -> WidgetRef {
    WidgetsApp::new(AccentPalette::default().base)
        .title(APP_NAME)
        .debug_show_checked_mode_banner(false)
        .navigator_key(notes.navigator.clone())
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .home(MainPage {
            notes: notes.clone(),
        })
        .into_widget()
}

/// From now on, a click on a reminder opens its note, the click that launched the program among
/// them.
fn hear_reminder_clicks(app: &mut App, notes: &NotesApp) {
    let notes = notes.clone();
    app.set_notification_activated_handler(Some(Rc::new(
        move |app: &mut App, activation: NotificationActivation| {
            reminder_clicked(app, &notes, activation);
        },
    )));
}
