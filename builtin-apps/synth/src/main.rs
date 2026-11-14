//! Synth, an instrument to play at once: a keyboard for the mouse, for several fingers and for
//! the computer's keys, a handful of presets made with fundsp, the few controls that shape them,
//! and a scope of what plays. A Waft app in Fluent's look, Waft's WinUI controls, over
//! `waft-embedder-wasdk`; its sound is made in the program and written ahead of the platform's
//! audio graph through a stream input (`output`).
//!
//! The layers, as Waft's README asks: the sound and the rules as plain Rust (`pitch`, `tone`,
//! `control`, `key_envelope`, `presets`, `voice`, `layer`, `synth`, `pacing`, `trace`,
//! `held_keys`, `keyboard_layout`, `typing`); the state over them (`instrument`, with `output`
//! its sound, and `typed_keys`, the computer's keys played on it); and the widgets (`page`,
//! `keyboard_view`, `scope_view`, `controls_view`, `window`).
//!
//! ```text
//! cargo waos run -p synth --release
//! ```

// Waft's `StatefulWidget::State` takes `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod control;
mod controls_view;
mod held_keys;
mod instrument;
mod key_envelope;
mod keyboard_layout;
mod keyboard_view;
mod layer;
mod output;
mod pacing;
mod page;
mod pitch;
mod presets;
mod scope_view;
mod synth;
mod tone;
mod trace;
mod typed_keys;
mod typing;
mod voice;
mod window;

use std::rc::Rc;

use waft::{
    App, AppLifecycleListener, IntoWidget, PageRoute, PageRouteBuilder, RouteSettingsRef,
    WidgetRef, WidgetsApp, WindowConfig, run_app_with,
};
use waft_winui::{AccentPalette, install_icon_font};

use instrument::Instrument;
use page::Page;
use window::{APP_NAME, FIRST_SIZE, SynthWindow};

#[wasdk::main]
async fn main() {
    waft_embedder_wasdk::run(start).await;
}

/// Starts the app: the instrument, played by the computer's keys, its window with its one page,
/// and WinUI's icons.
fn start(app: &mut App) {
    let instrument = Instrument::new(app);
    typed_keys::play_typed_keys(&instrument, app);
    let_go_of_the_keys_out_of_reach(&instrument, app);
    let window = WindowConfig {
        title: APP_NAME.to_owned(),
        size: FIRST_SIZE,
        ..WindowConfig::default()
    };
    run_app_with(app, widgets_app(instrument), window);
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// Lets every key go once the app no longer has its user's input, or is out of sight: the
/// release of a key held then would never arrive, and its note would sound on. Notes the
/// instrument sustains play on, as their user asked.
fn let_go_of_the_keys_out_of_reach(instrument: &Instrument, app: &mut App) {
    let (unreached, unseen) = (instrument.clone(), instrument.clone());
    AppLifecycleListener::new(app)
        .on_inactive(app, Rc::new(move |app| unreached.drop_holders(app)))
        .on_hide(app, Rc::new(move |app| unseen.drop_holders(app)));
}

/// The app in Fluent's accent, its one page in the window.
fn widgets_app(instrument: Instrument) -> WidgetRef {
    let app = WidgetsApp::new(AccentPalette::default().base)
        .title(APP_NAME)
        .debug_show_checked_mode_banner(false)
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            PageRoute::as_page_route(route)
        })
        .home(Page::new(instrument));
    SynthWindow::new(app.into_widget()).into_widget()
}
