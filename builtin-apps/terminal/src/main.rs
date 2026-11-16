//! Terminal: a shell on a machine its user picks, in a pseudo-terminal of WaOS's machines
//! interface, as a Waft app over `wasdk-adapter-waft`. What a person uses to try what WaOS gives
//! a program that runs commands: a machine picked and kept, a command in a terminal with its
//! output, its input, its size and its end, the keyboard in full, the platform's monospace font,
//! the clipboard, and a window for each session.
//!
//! - **A session** is a shell and the screen its output draws ([`session`], [`shell`]): `vt100`
//!   keeps the grid and reads the escape sequences ([`screen`]); the keys, pasted text and the
//!   mouse are typed as the bytes a terminal sends ([`keys`], [`paste`], [`mouse`]).
//! - **The grid** is drawn a row at a time from runs of text in one style ([`row_picture`],
//!   [`frame_picture`], [`grid_painter`], [`run_cache`]), in cells measured from the font ([`font`],
//!   [`cell_metrics`], [`text_size`]) and the theme's colours ([`palette`]).
//! - **The view** types into the shell ([`typing`], [`chords`], [`keyboard`], [`text_entry`]), selects and
//!   scrolls with the pointer ([`pointing`], [`selection`], [`clicks`], [`wheel`]), and copies
//!   and pastes ([`clipboard_commands`]), in [`terminal_view`].
//! - **A window** shows one session ([`window`]): the page that asks for a machine, then the
//!   terminal ([`session_page`]), under the menu bar ([`menus`]).
//!
//! ```text
//! cargo waos run -p terminal --release
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod cell_metrics;
mod chords;
mod clicks;
mod clipboard_commands;
mod font;
mod frame_picture;
mod grid_painter;
mod keyboard;
mod keys;
mod menus;
mod mouse;
mod palette;
mod paste;
mod pointing;
mod row_picture;
mod run_cache;
mod screen;
mod selection;
mod session;
mod session_page;
mod shell;
mod terminal_view;
mod text_entry;
mod text_size;
mod typing;
mod wheel;
mod window;

use std::rc::Rc;

use waft_foundation::{App, Entity};
use waft_widgets::{
    IntoWidget, MultiViewApp, PageRouteBuilder, PlatformMenuBar, RouteSettingsRef, WidgetRef,
    WidgetsApp, run_widget,
};
use waft_winui::AccentPalette;

use crate::grid_painter::TextSettings;
use crate::menus::menu_bar;
use crate::session_page::SessionPage;
use crate::text_size::first_text_settings;
use crate::window::{APP_NAME, SessionWindow, open_window};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the text every window shares, a first window where the platform gives the
/// program none, and a session in each window that arrives, under the menu bar.
fn start(app: &mut App) {
    let text = app.new_entity(|_cx| first_text_settings());
    if app.platform().views().is_empty() {
        open_window(app);
    }
    let windows = MultiViewApp::new({
        let text = text.clone();
        move |app, _view| Some(session_window(app, &text))
    });
    let content = PlatformMenuBar::new(menu_bar(&text)).child(windows);
    run_widget(app, content.into_widget());
}

/// What a window shows: a session of its own, opened as the window arrives.
fn session_window(app: &mut App, text: &Entity<TextSettings>) -> WidgetRef {
    let session = session::open(app);
    let page = SessionPage {
        session: session.clone(),
        text: text.clone(),
    };
    SessionWindow {
        session,
        child: widgets_app(page),
    }
    .into_widget()
}

/// The window's app: the navigator its page is the one route of, in Fluent's accent.
fn widgets_app(page: SessionPage) -> WidgetRef {
    WidgetsApp::new(AccentPalette::default().base)
        .title(APP_NAME)
        .debug_show_checked_mode_banner(false)
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .home(page)
        .into_widget()
}
