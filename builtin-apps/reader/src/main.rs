//! Reader: a feed reader, as a Waft app in WinUI's Fluent look (`waft-winui`) over
//! `wasdk-adapter-waft`. What a person uses to try what WaOS gives a program of the network, of
//! pictures and of text: HTTP from a program that keeps drawing while it waits, the system's image
//! codecs, long lists and long text, links opened outside, notifications, work done while the
//! window is out of sight, and its own `/data` and `/cache`.
//!
//! - **Feeds** are followed by address, a feed's own or a site's that names one ([`adding`],
//!   [`feed_address`], [`feed_discovery`]), three at the first launch ([`starting_feeds`]), and
//!   unfollowed in the settings ([`settings_page`], [`unfollowing`]).
//! - **A refresh** fetches each feed's document once ([`refreshing`], [`fetching`], [`http`]) and
//!   reads it into the library ([`feed_document`], [`library`]), which the program's `/data`
//!   keeps ([`store`]); with no network, the lists show what it has and say so ([`notices`]).
//! - **The window** lists the feeds in a Fluent navigation view beside the items, or, at a
//!   phone's width, in a pane over them ([`main_page`], [`feed_list`], [`header`], [`content`],
//!   [`content_insets`], [`window`]);
//!   the items are rows with their pictures ([`item_list`], [`item_row`], [`picture_view`],
//!   [`pictures`], [`picture_cache`]).
//! - **An item** is read in the app, its HTML laid out as headings, paragraphs, lists, quotations,
//!   code and pictures ([`article`], [`article_html`], [`reading_page`], [`article_view`],
//!   [`rich_text`]); its links and its original open in the system's browser ([`opening_links`]).
//! - **A notification** tells of new items a refresh on the app's own schedule found, where the
//!   user turned that on ([`new_item_notices`]).
//!
//! ```text
//! cargo waos run -p reader
//! ```

// Waft's `State` methods take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod adding;
mod article;
mod article_html;
mod article_view;
mod bar_button;
mod browsing;
mod content;
mod content_insets;
mod dates;
mod feed;
mod feed_address;
mod feed_discovery;
mod feed_document;
mod feed_list;
mod fetching;
mod header;
mod http;
mod icons;
mod item_list;
mod item_row;
mod library;
mod main_page;
mod new_item_notices;
mod notices;
mod opening_links;
mod picture_cache;
mod picture_view;
mod pictures;
mod question;
mod reader_app;
mod reading_page;
mod refreshing;
mod rich_text;
mod safe_write;
mod settings;
mod settings_page;
mod showing;
mod starting_feeds;
mod store;
mod unfollowing;
mod window;

use std::rc::Rc;

use waft::ui::NotificationActivation;
use waft::{
    App, IntoWidget, PageRouteBuilder, RouteSettingsRef, WidgetRef, WidgetsApp, run_app_with,
};
use waft_winui::{AccentPalette, install_icon_font};

use crate::main_page::MainPage;
use crate::new_item_notices::notice_clicked;
use crate::reader_app::ReaderApp;
use crate::refreshing::refresh_regularly;
use crate::window::{APP_NAME, ReaderWindow};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the library the program's `/data` keeps, refreshed now and every half hour;
/// the pictures kept trimmed; a notification's click heard; and the window.
fn start(app: &mut App) {
    let reader = ReaderApp::open(app);
    picture_cache::trim();
    refresh_regularly(app, &reader);
    hear_notice_clicks(app, &reader);
    let window = ReaderWindow {
        child: widgets_app(&reader),
    };
    run_app_with(app, window.into_widget(), window::config());
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// The window's app: its navigator, which the dialogs are shown over and the popups in, with the
/// page as its one route, in Fluent's accent.
fn widgets_app(reader: &ReaderApp) -> WidgetRef {
    WidgetsApp::new(AccentPalette::default().base)
        .title(APP_NAME)
        .debug_show_checked_mode_banner(false)
        .navigator_key(reader.navigator.clone())
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .home(MainPage {
            reader: reader.clone(),
        })
        .into_widget()
}

/// From now on, a click on the app's notification brings its window forward, the click that
/// launched the program among them.
fn hear_notice_clicks(app: &mut App, reader: &ReaderApp) {
    let reader = reader.clone();
    app.set_notification_activated_handler(Some(Rc::new(
        move |app: &mut App, activation: NotificationActivation| {
            notice_clicked(app, &reader, &activation);
        },
    )));
}
