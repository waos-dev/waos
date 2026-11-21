//! A camera: the camera's picture live in the window, drawn through the look chosen among a few,
//! each a shader of the app's own run on its own WebGPU device over the frames the camera hands
//! out; a shutter that has the system take a photo, which is developed in the same look and kept
//! in the app's own `/data`; and the photos kept, viewed large, exported through the system's
//! save dialog and deleted. Where the system has several cameras, a button switches among them.
//! It says plainly why there is no picture while there is none: WaOS is asking the user for the
//! camera, the user refused it, or there is no camera; and starts by itself once there can be
//! one. The camera stops while the app is out of sight. A Waft app in Fluent's look, Waft's
//! WinUI controls, over `wasdk-adapter-waft`.
//!
//! ```text
//! waos run -p camera-app
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod camera_app;
mod camera_page;
mod controls;
mod darkroom;
mod dates;
mod developing;
mod exporting;
mod icons;
mod library;
mod live_picture;
mod live_view;
mod look_bar;
mod look_painter;
mod looks;
mod notice_bar;
mod permission;
mod photo_tile;
mod photos_page;
mod pictures;
mod roll;
mod sheet;
mod shooting;
mod shutter_button;
mod status_message;
mod viewfinder;
mod window;

use std::rc::Rc;

use waft::{
    App, AppLifecycleListener, Entity, IntoWidget, PageRouteBuilder, RouteSettingsRef, SizedBox,
    WidgetRef, WidgetsApp, WindowConfig, run_app_with,
};
use waft_winui::{AccentPalette, Theme, ThemeScope, install_icon_font};

use camera_app::CameraApp;
use camera_page::CameraPage;
use viewfinder::Viewfinder;
use window::CameraWindow;

/// The app's name, its window's title.
const APP_NAME: &str = "Camera";

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the camera starting, the photos kept listed, and the window, in the dark, as
/// a camera's is.
fn start(app: &mut App) {
    let camera = CameraApp::open(app);
    rest_while_out_of_sight(&camera.viewfinder, app);
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
        // Around the navigator, so its pages, dialogs and tooltips are dark as well.
        .builder(|_app, _context, navigator| in_the_dark(navigator.cloned()))
        .home(CameraWindow {
            child: CameraPage { camera }.into_widget(),
        })
        .into_widget();
    let window = WindowConfig {
        title: APP_NAME.to_owned(),
        size: [840.0, 640.0],
        ..WindowConfig::default()
    };
    run_app_with(app, page, window);
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// Stops the camera while the app is hidden, and starts it again once it shows.
fn rest_while_out_of_sight(viewfinder: &Entity<Viewfinder>, app: &mut App) {
    let (hidden, shown) = (viewfinder.clone(), viewfinder.clone());
    let lifecycle = AppLifecycleListener::new(app);
    lifecycle
        .on_hide(app, Rc::new(move |app| viewfinder::rest(app, &hidden)))
        .on_show(app, Rc::new(move |app| viewfinder::wake(app, &shown)));
}

/// `navigator`, the app's pages with their dialogs, in Fluent's dark theme. Each page paints its
/// own background.
fn in_the_dark(navigator: Option<WidgetRef>) -> WidgetRef {
    let pages = navigator.unwrap_or_else(|| SizedBox::new().into_widget());
    ThemeScope::new(Theme::Dark, pages).into_widget()
}
