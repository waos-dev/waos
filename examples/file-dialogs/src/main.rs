//! A program that picks in the platform's file dialogs, in order: a file to read, which it reads
//! and tries to write to; a file to change, asked writable, which it writes; where to save, which
//! it writes, then saves again safely, writing in its temporary folder and renaming onto the file
//! picked; and a folder, whose entries it counts. It says where its temporary folder is first.
//!
//! It prints each step as `what: result`.
//!
//! ```text
//! cargo build -p file-dialogs --target wasm32-wasip3
//! ```

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use wasdk::file_dialogs::{self, FileDialogs, FolderOptions, OpenOptions, SaveOptions};

#[wasdk::main]
async fn main() {
    println!("temporary folder: {}", temporary_folder().display());
    let Some(dialogs) = file_dialogs::get_file_dialogs() else {
        println!("file dialogs: none");
        return;
    };
    read_a_file(&dialogs).await;
    change_a_file(&dialogs).await;
    save_a_file(&dialogs).await;
    count_a_folder(&dialogs).await;
}

/// Picks a file to read, reads it, and tries to write to it, which the pick does not allow.
async fn read_a_file(dialogs: &FileDialogs) {
    let options = OpenOptions::new();
    options.add_type("Text", &["txt".to_owned()], &["text/plain".to_owned()]);
    let Some(path) = picked("open", dialogs.open_file(None, options).await) else {
        return;
    };
    println!("read: {}", fs::read_to_string(&path).unwrap_or_default());
    let written = fs::OpenOptions::new().append(true).open(&path).is_ok();
    println!("read file written to: {}", yes_or_no(written));
}

/// Picks a file to change, asking to write it, and writes a line onto its end.
async fn change_a_file(dialogs: &FileDialogs) {
    let options = OpenOptions::new();
    options.writable();
    let Some(path) = picked("open writable", dialogs.open_file(None, options).await) else {
        return;
    };
    let written = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(b", changed"));
    println!("changed: {}", yes_or_no(written.is_ok()));
}

/// Picks where to save, writes the file there, then saves it again safely: written in its
/// temporary folder and renamed onto the file picked, which replaces it whole.
async fn save_a_file(dialogs: &FileDialogs) {
    let options = SaveOptions::new();
    options.suggested_name("saved.txt");
    let Some(path) = picked("save", dialogs.save_file(None, options).await) else {
        return;
    };
    println!(
        "saved: {}",
        yes_or_no(fs::write(&path, "first save").is_ok())
    );
    let temporary = temporary_folder().join("saved.txt.new");
    let saved_safely =
        fs::write(&temporary, "second save").and_then(|()| fs::rename(&temporary, &path));
    println!("saved safely: {}", yes_or_no(saved_safely.is_ok()));
}

/// Picks a folder, and counts what is in it.
async fn count_a_folder(dialogs: &FileDialogs) {
    let options = FolderOptions::new();
    let Some(path) = picked("folder", dialogs.open_folder(None, options).await) else {
        return;
    };
    let entries = fs::read_dir(&path).map(Iterator::count);
    match entries {
        Ok(count) => println!("folder entries: {count}"),
        Err(error) => println!("folder entries: {error}"),
    }
}

/// The path picked, said as `what: path`; none, said, when the user cancelled or the dialog
/// failed.
fn picked(what: &str, answer: Result<Option<String>, file_dialogs::DialogError>) -> Option<String> {
    match answer {
        Ok(Some(path)) => {
            println!("{what}: {path}");
            Some(path)
        }
        Ok(None) => {
            println!("{what}: cancelled");
            None
        }
        Err(file_dialogs::DialogError::Failed(reason)) => {
            println!("{what}: failed ({reason})");
            None
        }
    }
}

/// The program's temporary folder, as `TMPDIR` names it: Rust's `std::env::temp_dir` has none on
/// `wasm32-wasip3`, where it panics.
fn temporary_folder() -> PathBuf {
    std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

fn yes_or_no(yes: bool) -> &'static str {
    if yes { "yes" } else { "no" }
}
