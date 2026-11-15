//! A program that asks for what it may use, and what a reader learns the `permissions` interface
//! from: it asks for a window, which a question is put over, and goes on without one where the
//! platform gives none; prints where each permission stands and follows the changes of each;
//! asks for notifications twice at once, which puts the question to its user once, and prints
//! each answer; asks for the network, which opens its page in the program's settings, and prints
//! where it stands once the user leaves the page; and returns once it hears its clipboard-write
//! switch change.
//!
//! ```text
//! cargo build -p permissions --target wasm32-wasip3
//! ```

use wasdk::permissions::{self, Permission, State};
use wasdk::views::{self, ViewChange};
use wasdk::windows::WindowConfig;

/// Each permission, by the name the program prints it under.
fn every_permission() -> [(&'static str, Permission); 7] {
    [
        ("network", permissions::network()),
        ("local-network", permissions::local_network()),
        ("clipboard-read", permissions::clipboard_read()),
        ("clipboard-write", permissions::clipboard_write()),
        ("notifications", permissions::notifications()),
        ("camera", permissions::camera()),
        ("microphone", permissions::microphone()),
    ]
}

#[wasdk::main]
async fn main() {
    open_window().await;
    let mut clipboard_write = None;
    for (name, permission) in every_permission() {
        println!("{name}: {}", said(permission.state()));
        let changes = permission.changes();
        if name == "clipboard-write" {
            clipboard_write = Some(changes);
        } else {
            wasdk::spawn(print_changes(name, changes));
        }
    }

    let notifications = permissions::notifications();
    let (first, second) = (notifications.request(), notifications.request());
    println!("notifications asked: {}", said(first.await));
    println!("notifications asked: {}", said(second.await));

    let network = permissions::network().request().await;
    println!("network after its settings: {}", said(network));

    let mut clipboard_write = clipboard_write.expect("every permission is read");
    if let Some(state) = clipboard_write.next().await {
        println!("clipboard-write changed: {}", said(state));
    }
}

/// Asks for a window and waits for it, or says why there is none.
async fn open_window() {
    let mut changes = views::view_changes();
    let config = WindowConfig::new();
    config.set_title("Permissions");
    if let Err(error) = views::request_view(&config, None) {
        println!("no window: {error}");
        return;
    }
    while let Some(change) = changes.next().await {
        match change {
            ViewChange::Added(_) => return println!("a window"),
            ViewChange::RequestFailed(failure) => return println!("no window: {}", failure.error),
            ViewChange::Removed(_) => {}
        }
    }
}

/// Prints each state `name` changes to.
async fn print_changes(name: &'static str, mut changes: wasdk::StreamReader<State>) {
    while let Some(state) = changes.next().await {
        println!("{name} changed: {}", said(state));
    }
}

/// Where a permission stands, in words.
fn said(state: State) -> &'static str {
    match state {
        State::NotAsked => "not asked",
        State::Granted => "granted",
        State::Denied => "denied",
    }
}
