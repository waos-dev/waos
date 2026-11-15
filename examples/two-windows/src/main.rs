//! An Waft app with two windows, each spinning an activity indicator, so that each asks for a
//! frame of its own view at every frame it draws: what the engine's tests hide one window of, to
//! see the other keep drawing while the hidden one draws nothing until it shows. Asking to close
//! either window quits it.
//!
//! ```text
//! cargo build -p two-windows --target wasm32-wasip2
//! cargo waos run -p two-windows
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::rc::Rc;

use waft_cupertino::CupertinoActivityIndicator;
use waft_embedder::{AppExitType, ViewRef, WindowConfig};
use waft_foundation::App;
use waft_services::ServicesBinding;
use waft_widgets::{Center, IntoWidget, MultiViewApp, run_widget};

/// The title of each window, in the order they are asked for.
const TITLES: [&str; 2] = ["first", "second"];

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        for title in TITLES {
            ask_for_window(app, title);
        }
        let windows = MultiViewApp::new(|_app, view| view.initial_data().is_some().then(spinner))
            .on_close_requested(Rc::new(quit));
        run_widget(app, windows.into_widget());
    })
    .await;
}

/// Asks for a window titled `title`, which carries its title as its initial data; none where the
/// platform makes none.
fn ask_for_window(app: &mut App, title: &str) {
    let config = WindowConfig {
        title: title.to_owned(),
        size: [320.0, 240.0],
        ..WindowConfig::default()
    };
    let _refused = app.platform().request_view(config, Some(title.to_owned()));
}

/// An activity indicator spinning at a window's middle.
fn spinner() -> waft_widgets::WidgetRef {
    let spinner = CupertinoActivityIndicator::new().radius(32.0);
    Center::new().child(spinner).into_widget()
}

/// Asks the app to quit, as Cmd-Q does.
fn quit(app: &mut App, _view: &ViewRef) {
    let services = ServicesBinding::instance(app);
    let _answer = services.exit_application(app, AppExitType::Cancelable, 0);
}
