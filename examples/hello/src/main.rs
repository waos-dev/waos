//! The smallest Waft app, in Cupertino: one page that says hello, in the window `run_app` asks
//! for, quitting when the user closes it. The website shows this file below its module docs, so
//! what it shows is what compiles.
//!
//! ```text
//! cargo waos build -p hello
//! cargo waos run -p hello
//! ```

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
        CupertinoPageScaffold::new(Center::new().child(Text::new("Hello"))).into_widget()
    }
}
