//! WaOS's inspector: a program holding the inspector role, which it asks for by calling
//! `wasdk::inspect`. It lists the programs on this device, those that used something guarded
//! lately first, and shows for the one chosen its report, what it did over the last seven days,
//! and its live panels, what it does as it happens (the inspector's panels, decisions 1, 2 and 14
//! in `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`). In Fluent's controls from
//! 600 logical points wide, the list beside the page of the program chosen; narrower, as on a
//! phone, in Cupertino's, the list, then a page of each program. A Waft app over
//! `wasdk-adapter-waft`, in Waft's three layers: the logic as plain Rust (`logic`), the state as
//! entities over it, one a feature (`state`), and thin widgets (`ui`).
//!
//! ```text
//! cargo waos run -p inspector
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

mod clock;
mod logic;
mod program_icon;
mod state;
mod ui;
mod words;

use waft::{
    App, BuildContext, Entity, IntoWidget, MediaQuery, StatelessWidget, WidgetRef, WindowConfig,
    run_app_with,
};
use waft_cupertino::install_cupertino_icon_font;
use waft_winui::install_icon_font;

use crate::state::page::ProgramPage;
use crate::state::programs::Programs;
use crate::ui::cupertino::CupertinoInspector;
use crate::ui::fluent::FluentInspector;

/// The width from which the app takes Fluent's layout, as WaOS's library does: Material 3's line
/// between its compact window class and the rest.
const FLUENT_FROM_WIDTH: f64 = 600.0;

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(start).await;
}

/// Starts the app: the device's programs read from now on, and the window.
fn start(app: &mut App) {
    let programs = Programs::open(app);
    let page = ProgramPage::open(app, &programs);
    let window = WindowConfig {
        title: words::APP_NAME.to_owned(),
        size: [1200.0, 760.0],
        ..WindowConfig::default()
    };
    run_app_with(app, LaidOut { page }.into_widget(), window);
    // After `run_app_with`, which makes the font collection the icons' fonts join.
    install_icon_font(app);
    install_cupertino_icon_font(app);
}

/// The app laid out for its window's width.
#[derive(Debug)]
struct LaidOut {
    page: Entity<ProgramPage>,
}

impl StatelessWidget for LaidOut {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let page = self.page.clone();
        if MediaQuery::size_of(app, context).width() < FLUENT_FROM_WIDTH {
            CupertinoInspector { page }.into_widget()
        } else {
            FluentInspector { page }.into_widget()
        }
    }
}
