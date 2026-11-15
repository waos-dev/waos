//! A program that watches a folder in its own `/tmp`, in order: it says what `TMPDIR` names and
//! that `/tmp` takes a file; opens `/tmp/watched` from the preopened root and watches it twice,
//! everything under it and its own entries alone; makes a file, writes it, renames it and removes
//! it, saying how each change came; writes a file in a folder beneath, which only the recursive
//! watch tells, and a file beside, which both tell; and says what a watch of the root and of a file
//! fail with.
//!
//! It prints each step as `what: result`, and `done` at the end.
//!
//! ```text
//! cargo build -p file-watching --target wasm32-wasip3
//! ```

use std::fs;
use std::io::Write as _;

use wasdk::StreamReader;
use wasdk::file_watching::{self, Change};
use wasdk::filesystem::{Descriptor, DescriptorFlags, ErrorCode, OpenFlags, PathFlags};
use wasdk::preopens;

/// How long each batch waits after its first change, in milliseconds.
const LATENCY_MS: u32 = 50;

/// A watch's batches of changes.
type Batches = StreamReader<Vec<Change>>;

#[wasdk::main]
async fn main() {
    say_what_tmp_is();
    fs::create_dir_all("/tmp/watched/sub").expect("a folder in /tmp");
    let root = root();
    let watched = root
        .open_at(
            PathFlags::empty(),
            "tmp/watched".to_owned(),
            OpenFlags::DIRECTORY,
            DescriptorFlags::READ,
        )
        .await
        .expect("/tmp/watched opens");
    let mut everything = file_watching::watch(&watched, true, LATENCY_MS)
        .await
        .expect("a watch");
    let mut own_entries = file_watching::watch(&watched, false, LATENCY_MS)
        .await
        .expect("a watch");
    change_a_file(&mut everything).await;
    change_beneath(&mut everything, &mut own_entries).await;
    say_what_cannot_be_watched(&root).await;
    println!("done");
}

/// Says what `TMPDIR` names, and whether `/tmp` takes a file.
fn say_what_tmp_is() {
    println!(
        "TMPDIR: {}",
        std::env::var("TMPDIR").unwrap_or_else(|_| "unset".to_owned())
    );
    fs::write("/tmp/written", "temporary").expect("/tmp takes a file");
    let read = fs::read_to_string("/tmp/written").expect("and gives it back");
    println!("tmp holds: {read}");
}

/// The root `/`, the folder the program is given, which its other folders open from.
fn root() -> Descriptor {
    preopens::get_directories()
        .into_iter()
        .find_map(|(folder, path)| (path == "/").then_some(folder))
        .expect("the root is preopened")
}

/// Makes a file, writes it, renames it and removes it, saying how each change came.
async fn change_a_file(batches: &mut Batches) {
    fs::write("/tmp/watched/note.txt", "one").expect("made");
    wait_for(batches, &["created note.txt"]).await;
    println!("created: note.txt");

    let mut note = fs::OpenOptions::new()
        .append(true)
        .open("/tmp/watched/note.txt")
        .expect("opened");
    note.write_all(b" two").expect("written");
    drop(note);
    wait_for(batches, &["modified note.txt"]).await;
    println!("modified: note.txt");

    fs::rename("/tmp/watched/note.txt", "/tmp/watched/kept.txt").expect("renamed");
    let mut seen = Vec::new();
    let how = loop {
        if holds(&seen, &["renamed note.txt kept.txt"]) {
            break "paired";
        }
        if holds(&seen, &["other note.txt", "other kept.txt"]) {
            break "each end";
        }
        seen.extend(next_batch(batches).await);
    };
    println!("renamed: note.txt to kept.txt, {how}");

    fs::remove_file("/tmp/watched/kept.txt").expect("removed");
    wait_for(batches, &["removed kept.txt"]).await;
    println!("removed: kept.txt");
}

/// Writes a file in the folder beneath, then one beside it: only the recursive watch tells the
/// first, and both the second.
async fn change_beneath(everything: &mut Batches, own_entries: &mut Batches) {
    fs::write("/tmp/watched/sub/deep.txt", "deep").expect("made");
    fs::write("/tmp/watched/marker.txt", "").expect("made");
    wait_for(everything, &["created sub/deep.txt", "created marker.txt"]).await;
    println!("recursive: sub/deep.txt and marker.txt");
    let told = wait_for(own_entries, &["created marker.txt"]).await;
    let told_deep = told.iter().any(|change| change.ends_with(" sub/deep.txt"));
    println!(
        "own entries: marker.txt{}",
        if told_deep {
            " and sub/deep.txt"
        } else {
            " alone"
        }
    );
}

/// Says what a watch of the root, above the mounts, and of a file fail with.
async fn say_what_cannot_be_watched(root: &Descriptor) {
    println!("watch the root: {}", refusal(root).await);
    let file = root
        .open_at(
            PathFlags::empty(),
            "tmp/written".to_owned(),
            OpenFlags::empty(),
            DescriptorFlags::READ,
        )
        .await
        .expect("the file opens");
    println!("watch a file: {}", refusal(&file).await);
}

/// What a watch of `folder` fails with.
async fn refusal(folder: &Descriptor) -> String {
    match file_watching::watch(folder, false, LATENCY_MS).await {
        Ok(_) => "watched".to_owned(),
        Err(ErrorCode::Unsupported) => "unsupported".to_owned(),
        Err(ErrorCode::NotDirectory) => "not-directory".to_owned(),
        Err(other) => format!("{other:?}"),
    }
}

/// The changes `batches` tells, each as [`told`] says it, until they hold each of `expected`.
async fn wait_for(batches: &mut Batches, expected: &[&str]) -> Vec<String> {
    let mut seen = Vec::new();
    while !holds(&seen, expected) {
        seen.extend(next_batch(batches).await);
    }
    seen
}

/// Whether `seen` holds each of `expected`.
fn holds(seen: &[String], expected: &[&str]) -> bool {
    expected
        .iter()
        .all(|change| seen.iter().any(|told| told == change))
}

/// The next batch, each change as [`told`] says it; a watch ends only when the program drops it.
async fn next_batch(batches: &mut Batches) -> Vec<String> {
    let batch = batches.next().await.expect("a watch goes on");
    batch.iter().map(told).collect()
}

/// A change as words: what happened, then the path or paths it names.
fn told(change: &Change) -> String {
    match change {
        Change::Created(path) => format!("created {path}"),
        Change::Modified(path) => format!("modified {path}"),
        Change::Removed(path) => format!("removed {path}"),
        Change::Renamed((from, to)) => format!("renamed {from} {to}"),
        Change::Other(path) => format!("other {path}"),
        Change::Rescan(Some(path)) => format!("rescan {path}"),
        Change::Rescan(None) => "rescan everything".to_owned(),
    }
}
