//! A program that opens what it names in the apps the system picks, then hears what its user
//! opens with it. In order: a web link, which opens freely; two links of `zoommtg`, a scheme of
//! its own, of which the first asks its user and the second opens as the user answered; two links
//! of `tel`, the same way; a link of a scheme no app opens; text that is no link; a note it writes
//! in `/data`, opened, then shown in the system's file manager; and a file it does not have. Then
//! it listens for what its user opens with it, until it is stopped: each file it reads and writes
//! to, and each link it says.
//!
//! It prints each step as `what: result`, the result `ok`, `denied`, `no app` or `failed (why)`.
//!
//! ```text
//! cargo build -p opener --target wasm32-wasip2
//! ```

use std::fs;
use std::io::Write;

use wasdk::lifecycle;
use wasdk::opener::{self, OpenError, Opener};

/// The note it opens, in its own folder.
const NOTE: &str = "/data/note.txt";

/// The links it opens, in order.
const LINKS: [&str; 7] = [
    "https://example.com/",
    "zoommtg://zoom.us/join",
    "zoommtg://zoom.us/again",
    "tel:+15550100",
    "tel:+15550101",
    "nothing-opens:this",
    "not a link",
];

#[wasdk::main]
async fn main() {
    match opener::get_opener() {
        Some(opener) => {
            open_links(&opener).await;
            open_files(&opener).await;
        }
        None => println!("opener: none"),
    }
    hear_what_is_opened().await;
}

/// Opens each of [`LINKS`].
async fn open_links(opener: &Opener) {
    for link in LINKS {
        let opened = opener.open_link(link.to_owned()).await;
        println!("open {link}: {}", outcome(opened));
    }
}

/// Writes the note, opens it and shows it, then opens a file it does not have.
async fn open_files(opener: &Opener) {
    if let Err(error) = fs::write(NOTE, "a note") {
        println!("note: {error}");
        return;
    }
    let opened = opener.open_file(NOTE.to_owned()).await;
    println!("open file {NOTE}: {}", outcome(opened));
    let revealed = opener.reveal_file(NOTE.to_owned()).await;
    println!("reveal file {NOTE}: {}", outcome(revealed));
    let missing = opener.open_file("/data/missing.txt".to_owned()).await;
    println!("open file /data/missing.txt: {}", outcome(missing));
}

/// Reads each file its user opens with it and writes to it, and says each link, until it is
/// stopped.
async fn hear_what_is_opened() {
    let mut requests = lifecycle::open_requests();
    println!("listening for open requests");
    while let Some(request) = requests.next().await {
        for path in request.files {
            let text = fs::read_to_string(&path).unwrap_or_else(|error| error.to_string());
            println!("opened file {path}: {text}");
            let written = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .and_then(|mut file| file.write_all(b", seen"));
            println!(
                "opened file {path} written to: {}",
                yes_or_no(written.is_ok())
            );
        }
        for link in request.links {
            println!("opened link: {link}");
        }
    }
}

fn yes_or_no(yes: bool) -> &'static str {
    if yes { "yes" } else { "no" }
}

/// What a call answered, in a word or two.
fn outcome(answer: Result<(), OpenError>) -> String {
    match answer {
        Ok(()) => "ok".to_owned(),
        Err(OpenError::Denied) => "denied".to_owned(),
        Err(OpenError::NoApp) => "no app".to_owned(),
        Err(OpenError::Failed(why)) => format!("failed ({why})"),
    }
}
