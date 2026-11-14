//! A program with pages to go back through, as an app with a navigator has: each press on its view
//! goes a page on, and the user's Back a page back. It says it handles Back while it is past its
//! first page, and no longer once it is back on it, so a Back there is the platform's own. It uses
//! the view it was given, or asks for one where it was given none; prints the page it is on and
//! whether it handles Back at each change; and ends when the user asks to close its view.
//!
//! ```text
//! cargo build -p back --target wasm32-wasip2
//! ```

use wasdk::views::{self, PointerChange, View, ViewChange, ViewEvent};
use wasdk::windows::WindowConfig;

#[wasdk::main]
async fn main() -> Result<(), String> {
    let view = the_view().await?;
    let Some(mut events) = view.events() else {
        return Err("its view's events were taken".to_owned());
    };
    let mut pages = Pages::default();
    pages.say();
    while let Some(event) = events.next().await {
        match event {
            ViewEvent::Pointer(pointer) if pointer.change == PointerChange::Down => {
                pages.go_on(&view);
            }
            ViewEvent::PopRoute => pages.go_back(&view),
            ViewEvent::CloseRequested => match view.window() {
                Some(window) => window.close(),
                None => break,
            },
            _ => {}
        }
    }
    Ok(())
}

/// The view the program was given, or else the one it asks for.
async fn the_view() -> Result<View, String> {
    // The stream is opened before the list is read, so a view added between the two is not missed.
    let mut changes = views::view_changes();
    if let Some(given) = views::views().into_iter().next() {
        return Ok(given);
    }
    let config = WindowConfig::new();
    config.set_title("Back");
    views::request_view(&config, None).map_err(|error| format!("no view: {error}"))?;
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(view) => return Ok(view),
            ViewChange::RequestFailed(failure) => {
                return Err(format!("no view: {}", failure.error));
            }
            ViewChange::Removed(_) => {}
        }
    }
    Err("no view came".to_owned())
}

/// How many pages past the first the program is.
#[derive(Default)]
struct Pages(u32);

impl Pages {
    /// A page on: from the first, it has one to go back to.
    fn go_on(&mut self, view: &View) {
        self.0 += 1;
        if self.0 == 1 {
            view.set_handles_back(true);
        }
        self.say();
    }

    /// A page back: on the first again, it has none to go back to.
    fn go_back(&mut self, view: &View) {
        println!("back");
        self.0 = self.0.saturating_sub(1);
        if self.0 == 0 {
            view.set_handles_back(false);
        }
        self.say();
    }

    fn say(&self) {
        println!("page {}, handles back: {}", self.0, self.0 > 0);
    }
}
