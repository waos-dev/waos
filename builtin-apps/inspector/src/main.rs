//! WaOS's inspector: a program holding the inspector role, which it asks for by calling
//! `wasdk::inspect`. It lists the programs on this device, those that used something guarded
//! lately first, each with its icon, version, whether it runs, when it last used something and
//! what, and says for the one chosen what it did over the last seven days, in plain words, a
//! section per capability (the inspector's first panels, decisions 1 and 2, in
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`). In Fluent's controls from
//! 600 logical points wide, the list beside what the program chosen did; narrower, as on a phone,
//! in Cupertino's, the list, then a page of each program. A Waft app over `wasdk-adapter-waft`.
//!
//! ```text
//! cargo waos run -p inspector
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod activity;
mod cupertino;
mod fluent;
mod inspector;
mod program_icon;
mod words;

use waft::{
    App, BuildContext, Entity, IntoWidget, MediaQuery, StatelessWidget, WidgetRef, WindowConfig,
    run_app_with,
};
use waft_winui::install_icon_font;

use crate::cupertino::CupertinoInspector;
use crate::fluent::FluentInspector;
use crate::inspector::Inspector;

/// The width from which the app takes Fluent's layout, as WaOS's library does: Material 3's line
/// between its compact window class and the rest.
const FLUENT_FROM_WIDTH: f64 = 600.0;

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the device's programs read from now on, and the window.
fn start(app: &mut App) {
    let inspector = Inspector::open(app);
    let window = WindowConfig {
        title: words::APP_NAME.to_owned(),
        size: [980.0, 680.0],
        ..WindowConfig::default()
    };
    run_app_with(app, LaidOut { inspector }.into_widget(), window);
    // After `run_app_with`, which makes the font collection the icons' font joins.
    install_icon_font(app);
}

/// The app laid out for its window's width.
#[derive(Debug)]
struct LaidOut {
    inspector: Entity<Inspector>,
}

impl StatelessWidget for LaidOut {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let inspector = self.inspector.clone();
        if MediaQuery::size_of(app, context).width() < FLUENT_FROM_WIDTH {
            CupertinoInspector { inspector }.into_widget()
        } else {
            FluentInspector { inspector }.into_widget()
        }
    }
}
