//! The app's one view: the one the platform gave, as a phone gives a program its tab's, or the
//! window asked for where it gave none.

use wasdk::views::{self, View, ViewChange};
use wasdk::windows::{Size, WindowConfig};

const TITLE: &str = "Model Viewer";
const SIZE: Size = Size {
    width: 1000.0,
    height: 680.0,
};
/// The smallest window the panel and a model both fit in.
const SMALLEST: Size = Size {
    width: 360.0,
    height: 420.0,
};

/// The view the app draws in.
pub async fn open() -> Result<View, String> {
    // Opened before the request, so the view that arrives for it is not missed.
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title(TITLE);
    config.set_size(SIZE);
    views::request_view(&config, None).map_err(|error| format!("no window: {error}"))?;
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => {
                limit_size(&view);
                return Ok(view);
            }
            ViewChange::RequestFailed(failure) => {
                return Err(format!("no window: {}", failure.error));
            }
            ViewChange::Removed(_) => {}
        }
    }
    Err("the views ended before the window arrived".into())
}

fn limit_size(view: &View) {
    if let Some(limits) = view.window().and_then(|window| window.size_limits()) {
        limits.set_min_size(Some(SMALLEST));
    }
}

/// The window's title for the model shown.
pub fn set_title(view: &View, model: &str) {
    if let Some(window) = view.window() {
        window.set_title(&format!("{model} — {TITLE}"));
    }
}
