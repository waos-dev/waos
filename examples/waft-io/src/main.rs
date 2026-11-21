//! A Waft app that uses Waft's own file system and HTTP client, over `wasdk-adapter-waft`: it
//! fetches a page through a redirect with `app.platform().http()` and says its status and how many
//! bytes came; shows
//! a picture from its URL with `NetworkImage`, which fetches through the same client; writes,
//! reads and lists files in `/data` with `app.platform().files()`, as a program's `std::fs` would;
//! and, on a press, asks its user for a file and reads it, a path through `Files::read` and a
//! handle or a stream by itself.
//!
//! Waft's client follows redirects, streams both ways, and passes WASI's timeouts and errors
//! through; the adapter implements it over WASI 0.3's `wasi:http` and `wasi:filesystem`, which
//! WaOS serves under the program's network and file settings.
//!
//! It prints each answer as `what: result`, so `waos run -p waft-io` shows them.
//!
//! ```text
//! cargo build -p waft-io --target wasm32-wasip3
//! waos run -p waft-io
//! ```

use std::path::Path;
use std::rc::Rc;

use http::{Method, Request};
use waft::{
    App, Column, EdgeInsetsGeometry, Image, IntoWidget, Listener, MainAxisAlignment, MainAxisSize,
    Padding, Text, WidgetsApp, WindowConfig, run_app_with,
};
use waft_embedder::{Body, OpenOptions, PickedFile, SendOptions};
use waft_painting::{ImageProviderRef, NetworkImage};
use waft_winui::{AccentPalette, Button, install_icon_font};

/// The page the app fetches and prints: a redirect the client follows to a page.
const PAGE: &str = "https://httpbin.org/redirect/1";

/// The picture it shows, fetched by Waft's own image provider through the same client.
const PICTURE: &str = "https://httpbin.org/image/png";

/// The file it writes and reads in its own `/data`.
const NOTE: &str = "/data/waft-io.txt";

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        let window = WindowConfig {
            title: "Waft IO".to_owned(),
            size: [480.0, 480.0],
            ..WindowConfig::default()
        };
        let page = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .main_axis_alignment(MainAxisAlignment::Center)
            .children([
                Padding::new(EdgeInsetsGeometry::all(16.0))
                    .child(Text::new("Fetched, shown and read through Waft's own IO."))
                    .into_widget(),
                Image::new(Rc::new(NetworkImage::new(PICTURE)) as ImageProviderRef).into_widget(),
                Padding::new(EdgeInsetsGeometry::all(16.0))
                    .child(Button::new(Text::new("Open a file…"), Listener::new(pick)))
                    .into_widget(),
            ])
            .into_widget();
        // The app widget gives the page its text direction and Fluent's accent; with no `home`
        // and no `routes` it has no navigator, so no page route.
        let page_app = WidgetsApp::new(AccentPalette::default().base)
            .builder(move |_app, _context, _child| page.clone())
            .into_widget();
        run_app_with(app, page_app, window);
        install_icon_font(app);
        fetch(app, PAGE);
        round_trip_files(app);
    })
    .await;
}

/// Fetches `url` with Waft's client and says the status and how many bytes came.
fn fetch(app: &mut App, url: &'static str) {
    let platform = app.platform();
    let Some(client) = platform.http() else {
        println!("http: none");
        return;
    };
    let request = Request::builder()
        .method(Method::GET)
        .uri(url)
        .body(Body::empty())
        .expect("a well-formed request");
    let sending = client.send(request, SendOptions::new());
    app.spawn(async move |_cx| match sending.await {
        Ok(response) => {
            let status = response.status();
            match response.into_body().read_all().await {
                Ok(bytes) => println!("get {url}: {status}, {} bytes", bytes.len()),
                Err(error) => println!("get {url}: {status}, body failed: {error}"),
            }
        }
        Err(error) => println!("get {url}: failed: {error}"),
    });
}

/// Writes a note in `/data`, reads it back and lists the folder with Waft's file system.
fn round_trip_files(app: &mut App) {
    let platform = app.platform();
    let Some(files) = platform.files() else {
        println!("files: none");
        return;
    };
    let writing = files.write(Path::new(NOTE), b"written through Waft's file service");
    app.spawn(async move |cx| {
        match writing.await {
            Ok(()) => println!("write {NOTE}: done"),
            Err(error) => return println!("write {NOTE}: failed: {error}"),
        }
        let reading = cx.update(|app| {
            app.platform()
                .files()
                .map(|files| files.read(Path::new(NOTE)))
        });
        match reading {
            Some(reading) => match reading.await {
                Ok(bytes) => println!("read {NOTE}: {}", String::from_utf8_lossy(&bytes)),
                Err(error) => println!("read {NOTE}: failed: {error}"),
            },
            None => println!("read {NOTE}: no files"),
        }
        let listing = cx.update(|app| {
            app.platform()
                .files()
                .map(|files| files.list(Path::new("/data")))
        });
        match listing {
            Some(listing) => match listing.await {
                Ok(entries) => {
                    let names: Vec<&str> =
                        entries.iter().map(|entry| entry.name.as_str()).collect();
                    println!("list /data: {}", names.join(", "));
                }
                Err(error) => println!("list /data: failed: {error}"),
            },
            None => println!("list /data: no files"),
        }
    });
}

/// Asks the user for a file and reads it whole, whichever kind of pick it is.
fn pick(app: &mut App) {
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        println!("file dialogs: none");
        return;
    };
    let opening = dialogs.open_file(None, OpenOptions::default());
    app.spawn(async move |cx| match opening.await {
        Ok(Some(picked)) => {
            let reading = match picked {
                PickedFile::Path(path) => {
                    cx.update(|app| app.platform().files().map(|files| files.read(&path)))
                }
                PickedFile::Handle(handle) => Some(handle.read_all()),
                PickedFile::Stream(stream) => Some(stream.read_all()),
            };
            match reading {
                Some(reading) => match reading.await {
                    Ok(bytes) => println!("pick: {} bytes", bytes.len()),
                    Err(error) => println!("pick: failed: {error}"),
                },
                None => println!("pick: no files"),
            }
        }
        Ok(None) => println!("pick: cancelled"),
        Err(error) => println!("pick: failed: {error}"),
    });
}
