//! A new WaOS app in WinUI's look, which `cargo waos run` runs. The docs and guides are at
//! https://waos.app/docs.
// Waft's `StatefulWidget::State` takes `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::rc::Rc;

use waft::{
    App, BuildContext, Center, ColoredBox, IntoWidget, Listener, PageRouteBuilder,
    RouteSettingsRef, StatelessWidget, WidgetRef, WidgetsApp, WindowConfig, run_app_with,
};
use waft_winui::{AccentPalette, Button, ThemeResources, install_icon_font};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Asks for the app's window, and gives WinUI's controls their icons.
fn start(app: &mut App) {
    let window = WindowConfig {
        size: [1080.0, 780.0],
        ..WindowConfig::default()
    };
    run_app_with(app, widgets_app(), window);
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// The app's pages, in Fluent's accent, `Home` the first.
fn widgets_app() -> WidgetRef {
    WidgetsApp::new(AccentPalette::default().base)
        .page_route_builder(|app, settings, builder| {
            let route = PageRouteBuilder::new(
                app,
                Rc::new(move |app, context, _, _| builder(app, context)),
            )
            .settings(app, RouteSettingsRef::Settings(settings.clone()));
            route.into_erased()
        })
        .home(Home)
        .into_widget()
}

#[derive(Debug)]
struct Home;

impl StatelessWidget for Home {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let click = Listener::new(|_app| {});
        ColoredBox::new(background)
            .child(Center::new().child(Button::text("Hello, WaOS", click)))
            .into_widget()
    }
}
