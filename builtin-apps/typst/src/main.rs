//! Documents: a live document editor built on Typst, the typesetting system, as a Waft app in
//! WinUI's Fluent look (`waft-winui`) over `wasdk-adapter-waft`. A document is written in
//! Typst's markup on one side and shown as typeset pages on the other, set again as it is typed
//! by Typst's own compiler, which runs inside the program.
//!
//! - **Typesetting** ([`typesetting`]) is Typst's compiler over what it sees of the outside: the
//!   text, the files beside the document, the fonts the app carries. It runs on a worker, another
//!   instance of the program ([`typesetting_worker`]), so the window goes on answering while it
//!   does. [`pages`] asks it to run once typing pauses, and keeps the pages and the problems it
//!   answers.
//! - **The pages' pictures** ([`page_images`]) are drawn by Typst's renderer at the screen's
//!   density, on the worker, one page at a time, only the pages in sight ([`preview_pane`],
//!   [`page_sheet`]), at the zoom chosen ([`zoom`], [`zooming`]).
//! - **The text** is edited in a Fluent text box ([`editor_pane`]) whose controller draws it as
//!   marked-up source and underlines the problems ([`markup`]), listed beneath it
//!   ([`problem_list`]); [`editing`] follows its changes, and Tab indents ([`indenting`]).
//! - **The document's file** ([`document`]) is opened, saved and exported as a PDF through the
//!   system's dialogs ([`filing`], [`exporting`], [`safe_write`]), or handed over by the system
//!   ([`opening`]); the one open last opens again ([`last_document`]). The files beside it are
//!   read once its folder is given ([`folder_access`]).
//! - **The window** ([`window`], [`main_page`]) shows both sides, or one at a phone's width,
//!   between a command bar and a status bar ([`command_bar`], [`status_bar`]), under the menu
//!   bar ([`menus`], [`commands`], [`shortcuts`]); [`notices`] says what a command did, and
//!   [`quitting`] asks before changes are lost ([`question`]).
//!
//! ```text
//! cargo waos run -p documents --release
//! ```

// Waft's `State` methods take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod bar_button;
mod bundled_fonts;
mod command_bar;
mod commands;
mod document;
mod documents_app;
mod editing;
mod editor_font;
mod editor_pane;
mod exporting;
mod filing;
mod folder_access;
mod icons;
mod indenting;
mod last_document;
mod main_page;
mod markup;
mod menus;
mod notices;
mod opening;
mod page_images;
mod page_sheet;
mod pages;
mod preview_pane;
mod problem_list;
mod question;
mod quitting;
mod safe_write;
mod shortcuts;
mod status_bar;
mod text_file;
mod typesetting;
mod typesetting_worker;
mod viewing;
mod welcome;
mod window;
mod zoom;
mod zooming;

use std::rc::Rc;

use waft::{
    App, IntoWidget, PageRouteBuilder, PlatformMenuBar, RouteSettingsRef, WidgetRef, WidgetsApp,
    run_app_with,
};
use waft_winui::{AccentPalette, install_icon_font};

use crate::documents_app::DocumentsApp;
use crate::editing::follow_editor;
use crate::editor_font::install_editor_font;
use crate::main_page::MainPage;
use crate::menus::menu_bar;
use crate::opening::{hear_open_requests, open_at_launch};
use crate::quitting::answer_quits;
use crate::shortcuts::app_shortcuts;
use crate::typesetting_worker::{TypesettingWorker, start_typesetting};
use crate::window::{APP_NAME, DocumentWindow};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the typesetting worker, the editor followed, the document of the last run or
/// the starting one shown, quits answered and the files the system opens it with heard; and the
/// window, under the menu bar.
fn start(app: &mut App) {
    let (typesetting, queued) = TypesettingWorker::new();
    let documents = DocumentsApp::new(app, typesetting);
    start_typesetting(app, &documents, queued);
    follow_editor(app, &documents);
    open_at_launch(app, &documents);
    answer_quits(app, &documents);
    hear_open_requests(app, &documents);
    let window = DocumentWindow {
        documents: documents.clone(),
        child: widgets_app(app, &documents),
    };
    let content = PlatformMenuBar::new(menu_bar(app, &documents)).child(window);
    run_app_with(app, content.into_widget(), window::config());
    // After `run_app_with`, which makes the font collection the fonts join.
    install_icon_font(app);
    install_editor_font(app);
}

/// The window's app: its navigator, which the dialogs are shown over and the popups in, with the
/// page as its one route, in Fluent's accent, with the commands' keys as its shortcuts.
fn widgets_app(app: &App, documents: &DocumentsApp) -> WidgetRef {
    let page = MainPage {
        documents: documents.clone(),
    };
    WidgetsApp::new(AccentPalette::default().base)
        .title(APP_NAME)
        .debug_show_checked_mode_banner(false)
        .navigator_key(documents.navigator.clone())
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .shortcuts(app_shortcuts(app, documents))
        .home(page)
        .into_widget()
}
