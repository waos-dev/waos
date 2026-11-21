//! A video player with a library: the videos in the folders the user adds through the system's
//! folder dialog, in a grid of their thumbnails, titles and lengths, with how far each was
//! watched, by section and by search; a click plays one in the window under the familiar
//! controls, the picture letterboxed and its subtitles drawn in the user's caption style, and Esc
//! or the controls' Library goes back. It also opens a video file, or a link; it says plainly what
//! the player is doing; it lists the media's subtitles and audio to choose among; and it remembers
//! what it opened, resuming each where it was left. An Waft app in Fluent's look, Waft's WinUI
//! controls, over `wasdk-adapter-waft`, so it meets, in one app a person uses, the media players and media
//! assets, image codecs, file dialogs, menus, keys, caption style and window that WaOS gives a
//! program.
//!
//! ```text
//! waos run -p video-player
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod clock;
mod commands;
mod controls;
mod facts;
mod facts_cache;
mod finding;
mod folders;
mod icons;
mod keys;
mod library;
mod library_screen;
mod menu_bar;
mod opening;
mod playback;
mod recents;
mod screen;
mod sections;
mod source;
mod status;
mod text;
mod thumbnail;
mod thumbnail_cache;
mod tracks;
mod video;
mod video_tile;
mod video_types;
mod window;

use std::rc::Rc;

use waft_embedder::{AppExitResponse, WindowConfig};
use waft_foundation::{App, Entity, Task};
use waft_widgets::{
    AppLifecycleListener, GlobalKey, IntoWidget, PageRouteBuilder, RouteSettingsRef, SizedBox,
    WidgetRef, WidgetsApp, run_app_with,
};
use waft_winui::{AccentPalette, Theme, ThemeScope, install_icon_font};

use library::Library;
use menu_bar::PlayerMenuBar;
use playback::Playback;
use screen::Screen;
use window::APP_NAME;

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: nothing playing, the library of the folders added before; the window, its
/// menu bar, its keys and Esc; what the user opens with the app played; and where what plays is
/// kept as the app goes.
fn start(app: &mut App) {
    let playback = Playback::new(app);
    let library = Library::new(app);
    opening::play_what_is_opened_with_the_app(&playback, app);
    keep_where_it_is_as_the_app_goes(&playback, app);
    let navigator = GlobalKey::new();
    let screen = Screen::new(playback.clone(), library.clone());
    let page = WidgetsApp::new(AccentPalette::default().base)
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
        .navigator_key(navigator.clone())
        .shortcuts(keys::shortcuts(app, &playback))
        // Around the navigator, so its dialogs and menus are dark as well.
        .builder(|_app, _context, navigator| in_the_dark(navigator.cloned()))
        .home(screen)
        .into_widget();
    let window = WindowConfig {
        title: APP_NAME.to_owned(),
        size: [960.0, 600.0],
        ..WindowConfig::default()
    };
    run_app_with(
        app,
        PlayerMenuBar::new(playback, library, navigator, page).into_widget(),
        window,
    );
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// Keeps where what plays is as the app is hidden or asked to end, so the next run resumes
/// there.
fn keep_where_it_is_as_the_app_goes(playback: &Entity<Playback>, app: &mut App) {
    let (hidden, ending) = (playback.clone(), playback.clone());
    let lifecycle = AppLifecycleListener::new(app);
    lifecycle
        .on_hide(
            app,
            Rc::new(move |app| playback::keep_where_it_is(&hidden, app)),
        )
        .on_exit_requested(
            app,
            Rc::new(move |app| {
                playback::keep_where_it_is(&ending, app);
                Task::ready(AppExitResponse::Exit)
            }),
        );
}

/// `navigator`, the app's pages with their dialogs and menus, in Fluent's dark theme, as a
/// player is. Each page paints its own background.
fn in_the_dark(navigator: Option<WidgetRef>) -> WidgetRef {
    let pages = navigator.unwrap_or_else(|| SizedBox::new().into_widget());
    ThemeScope::new(Theme::Dark, pages).into_widget()
}
