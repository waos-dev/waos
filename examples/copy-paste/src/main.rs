//! An Waft app whose text goes through the platform's clipboard: a window with a text field,
//! whose ⌘C, ⌘X and ⌘V copy and paste as a native app's do, and a line copied as it starts
//! through Waft's clipboard, given as text, promised as HTML, and promised as a file, `line.txt`,
//! each made when someone pastes it. It prints the text on the clipboard at each change it hears,
//! read with Flutter's `Clipboard.getData`, `clipboard changed: copied by copy-paste` first, its
//! own copy. Asking to close its window quits it, and its embedder flushes the clipboard first,
//! so the HTML outlives it.
//!
//! ```text
//! cargo build -p copy-paste --target wasm32-wasip2
//! cargo waos run -p copy-paste
//! ```

// Waft's bindings take `self: Handle<Self>`.
#![feature(arbitrary_self_types)]

use std::future::ready;
use std::rc::Rc;

use waft_cupertino::{CupertinoApp, CupertinoPageScaffold, CupertinoTextField};
use waft_embedder::{ClipboardItem, Content, CopyOptions, FileSink, WindowConfig};
use waft_foundation::{App, Listenable, Listener};
use waft_services::{Clipboard, ServicesBinding};
use waft_widgets::{Center, EdgeInsetsGeometry, IntoWidget, Padding, run_app_with};

/// The line it copies as it starts.
const COPIED: &str = "copied by copy-paste";

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        print_clipboard_changes(app);
        copy_line(app);
        let window = WindowConfig {
            title: "Copy and paste".to_owned(),
            size: [420.0, 160.0],
            ..WindowConfig::default()
        };
        let field = CupertinoTextField::new()
            .placeholder("Type, copy and paste")
            .autofocus(true);
        let page = CupertinoPageScaffold::new(
            Center::new().child(Padding::new(EdgeInsetsGeometry::all(24.0)).child(field)),
        );
        run_app_with(app, CupertinoApp::new().home(page).into_widget(), window);
    })
    .await;
}

/// Copies the line: as text, given; as HTML, promised; and as a file, promised.
fn copy_line(app: &mut App) {
    let text = (
        "text/plain".to_owned(),
        Content::Data(COPIED.as_bytes().to_vec()),
    );
    let html = (
        "text/html".to_owned(),
        Content::Promise(Rc::new(|| {
            Box::pin(ready(Some(format!("<b>{COPIED}</b>").into_bytes())))
        })),
    );
    let file = (
        "text/plain".to_owned(),
        Content::FilePromise {
            name: "line.txt".to_owned(),
            produce: Rc::new(|mut sink: Box<dyn FileSink>| {
                Box::pin(async move { sink.write(COPIED.as_bytes()).await })
            }),
        },
    );
    let items = vec![
        ClipboardItem {
            representations: vec![text, html],
        },
        ClipboardItem {
            representations: vec![file],
        },
    ];
    let platform = app.platform();
    let Some(clipboard) = platform.clipboard() else {
        return println!("no clipboard");
    };
    let written = clipboard.write(items, CopyOptions::new());
    app.spawn(async move |_cx| {
        if let Err(error) = written.await {
            println!("copy refused: {error}");
        }
    });
}

/// From now on, prints the text on the clipboard each time the platform says it changed.
fn print_clipboard_changes(app: &mut App) {
    let changes = ServicesBinding::instance(app).clipboard(app);
    changes.add_listener(app, Listener::new(print_clipboard_text));
}

/// Prints the text on the clipboard, once read.
fn print_clipboard_text(app: &mut App) {
    let read = Clipboard::get_data(app, Clipboard::K_TEXT_PLAIN);
    app.spawn(async move |_cx| {
        let text = read.await.and_then(|data| data.text);
        println!("clipboard changed: {}", text.unwrap_or_default());
    });
}
