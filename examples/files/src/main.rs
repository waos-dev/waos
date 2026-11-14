//! A program that uses its files through `std::fs`, and what a reader learns WaOS's files from:
//! it writes, reads, lists and moves files in its own `/data` and a writable mount, lists the
//! folders above its mounts, is refused writing a read-only mount, leaving a mount by `..` or by
//! a symlink, and changing the folders above its mounts, finds nothing outside them, and waits
//! for a mount the user adds while it runs, then for its removal, reading on through a file it
//! opened before.
//!
//! It prints each result as `what: result`, an error by its kind. It expects the mounts its test
//! gives it (`crates/waos/tests/all/files.rs`): its own `/data` and `/cache`, `/mnt/docs`
//! read-only, holding `readme.txt` and symlinks to it and out of the mount, `/mnt/work`
//! writable, and `/later`, holding `hello.txt`, added and removed while it runs.
//!
//! ```text
//! cargo build -p files --target wasm32-wasip2
//! ```

use std::fmt::Display;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};

use wasdk::monotonic_clock;

/// How long the program waits between two looks at a mount it waits for, in nanoseconds.
const LOOK_EVERY: u64 = 10_000_000;

#[wasdk::main]
async fn main() {
    own_folder_and_a_writable_mount();
    folders_above_the_mounts();
    a_read_only_mount();
    escapes();
    outside_every_mount();
    changes_above_the_mounts();
    a_mount_added_and_removed_while_running().await;
}

fn own_folder_and_a_writable_mount() {
    fs::create_dir_all("/data/notes").unwrap();
    fs::write("/data/notes/hello.txt", "hello").unwrap();
    say("data read", fs::read_to_string("/data/notes/hello.txt"));
    say(
        "data stat",
        fs::metadata("/data/notes/hello.txt").map(|metadata| metadata.len()),
    );
    say("data list", names("/data/notes"));
    say(
        "rename across mounts",
        fs::rename("/data/notes/hello.txt", "/mnt/work/hello.txt").map(|()| "done"),
    );
    let appended = OpenOptions::new()
        .append(true)
        .open("/mnt/work/hello.txt")
        .and_then(|mut file| file.write_all(b" world"));
    say("work append", appended.map(|()| "done"));
    say("work read", fs::read_to_string("/mnt/work/hello.txt"));
}

fn folders_above_the_mounts() {
    say("root list", names("/"));
    say("mnt list", names("/mnt"));
    say(
        "mnt is a folder",
        fs::metadata("/mnt").map(|metadata| metadata.is_dir()),
    );
}

fn a_read_only_mount() {
    say("docs read", fs::read_to_string("/mnt/docs/readme.txt"));
    say("docs link inside", fs::read_to_string("/mnt/docs/link-in"));
    say(
        "docs write",
        fs::write("/mnt/docs/new.txt", "new").map(|()| "done"),
    );
    let opened = OpenOptions::new().write(true).open("/mnt/docs/readme.txt");
    say("docs open to write", opened.map(|_| "opened"));
    say(
        "docs remove",
        fs::remove_file("/mnt/docs/readme.txt").map(|()| "done"),
    );
}

fn escapes() {
    say(
        "escape by ..",
        fs::read_to_string("/mnt/docs/../../outside.txt"),
    );
    say(
        "escape above the root",
        fs::read_to_string("/../outside.txt"),
    );
    say(
        "escape by a symlink",
        fs::read_to_string("/mnt/docs/link-out"),
    );
    say(
        "escape by an absolute symlink",
        fs::read_to_string("/mnt/docs/link-abs"),
    );
}

fn outside_every_mount() {
    say("outside read", fs::read_to_string("/etc/passwd"));
    say("outside stat", fs::metadata("/nowhere").map(|_| "found"));
    say("outside list", names("/mnt/nothing"));
}

fn changes_above_the_mounts() {
    say("root write", fs::write("/new.txt", "new").map(|()| "done"));
    say(
        "root make a folder",
        fs::create_dir_all("/new/folder").map(|()| "done"),
    );
    say("data make again", fs::create_dir("/data").map(|()| "done"));
    say("mnt remove", fs::remove_dir("/mnt").map(|()| "done"));
}

/// Waits for `/later`, reads it, then waits for it to go and reads on through the file it kept.
async fn a_mount_added_and_removed_while_running() {
    say("later read", fs::read_to_string("/later/hello.txt"));
    let mut kept = loop {
        if let Ok(file) = File::open("/later/hello.txt") {
            break file;
        }
        monotonic_clock::wait_for(LOOK_EVERY).await;
    };
    say("later read", fs::read_to_string("/later/hello.txt"));
    while fs::metadata("/later").is_ok() {
        monotonic_clock::wait_for(LOOK_EVERY).await;
    }
    say("later gone", fs::metadata("/later").map(|_| "found"));
    let mut contents = String::new();
    say(
        "later read through the kept file",
        kept.read_to_string(&mut contents).map(|_| contents.clone()),
    );
}

/// The names in the folder at `path`, in order.
fn names(path: &str) -> io::Result<String> {
    let mut names = fs::read_dir(path)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<io::Result<Vec<String>>>()?;
    names.sort();
    Ok(names.join(", "))
}

/// Prints `what: result`, an error by its kind.
fn say(what: &str, result: io::Result<impl Display>) {
    match result {
        Ok(value) => println!("{what}: {value}"),
        Err(error) => println!("{what}: {:?}", error.kind()),
    }
}
