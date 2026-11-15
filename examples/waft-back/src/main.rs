//! A Waft app with pages to go back through, over `wasdk-adapter-waft`: a tap anywhere on a page
//! pushes the next onto its navigator, and the user's Back pops it. It writes no code for Back:
//! Waft's app tells the platform whether its navigator can pop, which the platform says on its
//! view as `set-handles-back`, and the platform's `pop-route` reaches the navigator.
//!
//! It prints each page as it is built, `page N`, the first being 0.
//!
//! ```text
//! cargo build -p waft-back --target wasm32-wasip3
//! cargo waos run -p waft-back
//! ```

use std::rc::Rc;

use waft_cupertino::{CupertinoApp, CupertinoPageRoute};
use waft_embedder::{Color, WindowConfig};
use waft_foundation::{App, Listener};
use waft_rendering::HitTestBehavior;
use waft_widgets::{
    BuildContext, Builder, ColoredBox, GestureDetector, IntoWidget, Navigator, Route, WidgetRef,
    run_app_with,
};

/// What every page is drawn in.
const BACKGROUND: Color = Color::new(0xFFFFFFFF);

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        let window = WindowConfig {
            title: "Waft back".to_owned(),
            size: [320.0, 240.0],
            ..WindowConfig::default()
        };
        let pages = CupertinoApp::new().home(Builder::new(|app, context| page(app, context, 0)));
        run_app_with(app, pages.into_widget(), window);
    })
    .await;
}

/// The page `number` pages past the first, which a tap anywhere on takes a page on.
fn page(_app: &mut App, context: BuildContext, number: u32) -> WidgetRef {
    println!("page {number}");
    let next = Listener::new(move |app| {
        let route = CupertinoPageRoute::new(
            app,
            Rc::new(move |app, context| page(app, context, number + 1)),
        );
        let _popped = Navigator::push(app, context, Route::as_route(route));
    });
    GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(next)
        .child(ColoredBox::new(BACKGROUND))
        .into_widget()
}
