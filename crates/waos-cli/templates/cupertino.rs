//! A new WaOS app in Cupertino's look, which `cargo waos run` runs. The docs and guides are at
//! https://waos.app/docs.
// Waft's `StatefulWidget::State` takes `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use waft::{App, BuildContext, Center, IntoWidget, StatelessWidget, Text, WidgetRef, run_app};
use waft_cupertino::{CupertinoApp, CupertinoPageScaffold};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| run_app(app, CupertinoApp::new().home(Home).into_widget())).await;
}

#[derive(Debug)]
struct Home;

impl StatelessWidget for Home {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        CupertinoPageScaffold::new(Center::new().child(Text::new("Hello, WaOS"))).into_widget()
    }
}
