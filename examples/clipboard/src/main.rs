//! A program that uses the clipboard, and what a reader learns the `clipboard` interface from: it
//! asks for a window, waits for what is on the clipboard to change while the window has the
//! focus, and reads the first item in its first type; then copies two items, text given at once
//! with its HTML promised and its RTF promised but never made whole, and a file in its `/data`
//! beside a picture that exists only once pasted; and answers each paste of a promised form or
//! file, in a task of its own, until another copy replaces its own. Asking to close its window
//! ends it, once it has flushed the clipboard, so what its copy promises outlives it.
//!
//! It prints each step as `what: result`, a refusal by the error's case.
//!
//! ```text
//! cargo build -p clipboard --target wasm32-wasip2
//! ```

use wasdk::clipboard::{self, Clipboard, ClipboardError, Content, CopyOptions, Request, Source};
use wasdk::views::{self, View, ViewChange, ViewEvent};
use wasdk::windows::WindowConfig;
use wasdk::{StreamReader, wit_future, wit_stream};

/// How long the copy stays on the clipboard: a minute, in nanoseconds.
const EXPIRY: u64 = 60_000_000_000;

#[wasdk::main]
async fn main() {
    let Some(clipboard) = clipboard::get_clipboard() else {
        println!("no clipboard");
        return;
    };
    // Before the window, so no change made once it is there is missed.
    let mut changes = clipboard.changes();
    let window = open_window().await;
    if let Some(items) = changes.next().await {
        let types: Vec<String> = items.iter().map(|item| item.types().join(",")).collect();
        println!("changed: {}", types.join(" "));
        read_first(&items).await;
    }
    let Some(source) = copy(&clipboard).await else {
        return;
    };
    wasdk::spawn(answer_pastes(source));
    if let Some(window) = window {
        close_requested(&window).await;
    }
    // While the task above still answers, so the promised forms outlive the program.
    clipboard.flush().await;
    println!("flushed");
}

/// Asks for a window and waits for it.
async fn open_window() -> Option<View> {
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Clipboard");
    if let Err(error) = views::request_view(&config, None) {
        println!("no window: {error}");
        return None;
    }
    while let Some(change) = changes.next().await {
        if let ViewChange::Added(view) = change {
            println!("a window");
            return Some(view);
        }
    }
    None
}

/// Waits until the user asks to close `window`.
async fn close_requested(window: &View) {
    let Some(mut events) = window.events() else {
        return;
    };
    while let Some(event) = events.next().await {
        if let ViewEvent::CloseRequested = event {
            return;
        }
    }
}

/// Reads the first item in its first type.
async fn read_first(items: &[clipboard::ClipboardItem]) {
    let Some(item) = items.first() else {
        return println!("read: nothing on the clipboard");
    };
    let mime = item.types().first().cloned().unwrap_or_default();
    match item.read(mime).await {
        Ok(content) => println!(
            "read: {}",
            String::from_utf8_lossy(&content.collect().await)
        ),
        Err(error) => println!("read refused: {}", case(&error)),
    }
}

/// Copies its two items: the source that answers their promised forms, unless refused.
async fn copy(clipboard: &Clipboard) -> Option<Source> {
    std::fs::write("/data/note.txt", "a note").expect("its own /data");
    let text = vec![
        (
            "text/plain".to_owned(),
            Content::Data(stream_of(b"copied text".to_vec())),
        ),
        ("text/html".to_owned(), Content::Promise),
        ("text/rtf".to_owned(), Content::Promise),
    ];
    let files = vec![
        (
            "text/plain".to_owned(),
            Content::File("/data/note.txt".to_owned()),
        ),
        (
            "image/png".to_owned(),
            Content::FilePromise("picture.png".to_owned()),
        ),
    ];
    let options = CopyOptions::new();
    options.sensitive();
    options.expires_after(EXPIRY);
    match clipboard.write(vec![text, files], options).await {
        Ok(source) => {
            println!("written");
            Some(source)
        }
        Err(error) => {
            println!("write refused: {}", case(&error));
            None
        }
    }
}

/// Answers each paste of `source`'s promised forms and file, until another copy replaces it.
async fn answer_pastes(source: Source) {
    let mut requests = source.requests();
    while let Some(request) = requests.next().await {
        answer(request);
    }
    println!("replaced");
}

/// Answers one paste: its content, and whether it was made whole. The RTF breaks half-way.
fn answer(request: Request) {
    let mime = request.type_();
    println!("asked for: {} {mime}", request.item());
    let (content, made): (&[u8], Result<(), String>) = match mime.as_str() {
        "text/html" => (b"<b>copied text</b>", Ok(())),
        "text/rtf" => (b"{\\rtf1", Err("no RTF here".to_owned())),
        _ => (b"PNG", Ok(())),
    };
    let (mut writer, content_reader) = wit_stream::new::<u8>();
    let (done, done_reader) = wit_future::new::<Result<(), String>>(unanswered);
    request.provide(content_reader, done_reader);
    let content = content.to_vec();
    wasdk::spawn(async move {
        writer.write_all(content).await;
        drop(writer);
        let _ = done.write(made).await;
    });
}

/// What a `done` dropped unwritten says.
fn unanswered() -> Result<(), String> {
    Err("dropped unanswered".to_owned())
}

/// `bytes`, as a stream the program gives.
fn stream_of(bytes: Vec<u8>) -> StreamReader<u8> {
    let (mut writer, reader) = wit_stream::new::<u8>();
    wasdk::spawn(async move {
        writer.write_all(bytes).await;
    });
    reader
}

/// The error's case, as the WIT names it.
fn case(error: &ClipboardError) -> String {
    match error {
        ClipboardError::Changed => "changed".to_owned(),
        ClipboardError::NotFound => "not-found".to_owned(),
        ClipboardError::Denied => "denied".to_owned(),
        ClipboardError::Unsupported => "unsupported".to_owned(),
        ClipboardError::Failed(reason) => format!("failed ({reason})"),
    }
}
