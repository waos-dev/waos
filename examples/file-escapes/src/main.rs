//! A program that tries to leave its mounts through `wasi:filesystem`'s calls on a folder it
//! opened, which `std::fs` has no form of: it makes a symlink in its own `/data` leading out of it
//! and one leading inside, and names each with a `/` after it in every call that changes a folder
//! (GHSA-m9f7-9w4p-gv38, where cap-std's rename followed `link/` out); reads the status of a
//! symlink to a folder and of a file, each named with a `/` after it, which asks for a folder; and,
//! from a writable mount's folder, reaches for the read-only mount beside it by `..`, by a symlink
//! it makes and by one the user left there, and renames into and out of it from another folder.
//! Then it names the same with a `/` after them through `std::fs`, whose paths go from the root
//! through the mount table, which keeps their `/`.
//!
//! It prints each result as `what: result`, an error by WASI's name for it, or by its kind for
//! `std`'s, and `done` at the end.
//! It expects the mounts its test gives it (`crates/waos/tests/all/file_escapes.rs`): its own
//! `/data`, with a folder `outside` beside it on the machine, `/mnt/work` writable, holding
//! `docs-abs`, a symlink to the docs folder's path on the machine, and `/mnt/docs` read-only beside
//! it, holding `readme.txt`.
//!
//! ```text
//! cargo build -p file-escapes --target wasm32-wasip2
//! ```

use std::fs;

use wasip2::filesystem::preopens;
use wasip2::filesystem::types::{
    Descriptor, DescriptorFlags, DescriptorType, ErrorCode, OpenFlags, PathFlags,
};

/// The symlinks the program makes in its `/data`: out of it, to the folder beside it on the
/// machine, and to a folder inside it.
const LINKS: [(&str, &str); 2] = [("link-out", "../outside"), ("link-in", "inside")];

#[wasdk::main]
async fn main() {
    let root = root();
    let data = open_folder(&root, "data");
    let work = open_folder(&root, "mnt/work");
    let docs = open_folder(&root, "mnt/docs");
    make_what_the_calls_name(&data);
    names_ending_in_a_slash(&data);
    statuses_of_names_ending_in_a_slash(&data);
    from_a_writable_mount_into_a_read_only_one(&work, &docs);
    names_ending_in_a_slash_from_the_root();
    println!("done");
}

/// The one folder the program is given, `/`.
fn root() -> Descriptor {
    let (root, path) = preopens::get_directories()
        .into_iter()
        .next()
        .expect("the program is given its root");
    assert_eq!(path, "/");
    root
}

/// The folder at `path` from `root`, opened for its paths' calls.
fn open_folder(root: &Descriptor, path: &str) -> Descriptor {
    root.open_at(
        PathFlags::SYMLINK_FOLLOW,
        path,
        OpenFlags::DIRECTORY,
        DescriptorFlags::READ | DescriptorFlags::MUTATE_DIRECTORY,
    )
    .unwrap_or_else(|error| panic!("{path} opens: {error}"))
}

/// In `/data`, a folder with a file in it, an empty folder, a file and the symlinks; and in
/// `/mnt/work`, a note.
fn make_what_the_calls_name(data: &Descriptor) {
    fs::create_dir_all("/data/inside").unwrap();
    fs::write("/data/inside/kept.txt", "inside").unwrap();
    fs::create_dir_all("/data/folder").unwrap();
    fs::write("/data/file.txt", "a file").unwrap();
    for (link, leads_to) in LINKS {
        say(&format!("make {link}"), data.symlink_at(leads_to, link));
    }
    say(
        "make link-to-folder",
        data.symlink_at("folder", "link-to-folder"),
    );
    fs::write("/mnt/work/note.txt", "a note").unwrap();
}

/// Each call that changes a folder, naming each symlink with a `/` after it.
fn names_ending_in_a_slash(data: &Descriptor) {
    for (link, _) in LINKS {
        let named = format!("{link}/");
        let named = named.as_str();
        say(
            &format!("rename from {named}"),
            data.rename_at(named, data, "moved"),
        );
        say(
            &format!("rename a folder onto {named}"),
            data.rename_at("folder", data, named),
        );
        say(
            &format!("rename a file onto {named}"),
            data.rename_at("file.txt", data, named),
        );
        say(
            &format!("create-directory {named}"),
            data.create_directory_at(named),
        );
        say(
            &format!("remove-directory {named}"),
            data.remove_directory_at(named),
        );
        say(&format!("unlink-file {named}"), data.unlink_file_at(named));
        say(
            &format!("link from {named}"),
            data.link_at(PathFlags::empty(), named, data, "linked"),
        );
        say(
            &format!("link onto {named}"),
            data.link_at(PathFlags::empty(), "file.txt", data, named),
        );
        say(
            &format!("symlink onto {named}"),
            data.symlink_at("file.txt", named),
        );
    }
}

/// The status of a symlink to a folder and of a file, each named with a `/` after it, which asks
/// for a folder, without following a symlink the path ends in and following it.
fn statuses_of_names_ending_in_a_slash(data: &Descriptor) {
    for path in [
        "link-to-folder",
        "link-to-folder/",
        "file.txt/",
        "file.txt/.",
    ] {
        for (follow, flags) in [
            ("status", PathFlags::empty()),
            ("status followed", PathFlags::SYMLINK_FOLLOW),
        ] {
            let status = data.stat_at(flags, path).map(|status| kind(status.type_));
            say(&format!("{follow} {path}"), status);
        }
    }
}

/// From `/mnt/work`'s folder, every way into `/mnt/docs` beside it: by `..`, by a symlink the
/// program makes, by one with the folder's path on the machine, and by renaming from one folder
/// into the other; and the same by `std`, from the root.
fn from_a_writable_mount_into_a_read_only_one(work: &Descriptor, docs: &Descriptor) {
    say("make to-docs", work.symlink_at("../docs", "to-docs"));
    for path in ["../docs", "to-docs", "docs-abs"] {
        say(
            &format!("create {path}/new.txt"),
            work.open_at(
                PathFlags::SYMLINK_FOLLOW,
                &format!("{path}/new.txt"),
                OpenFlags::CREATE,
                DescriptorFlags::WRITE,
            )
            .map(|_| "opened"),
        );
        say(
            &format!("create-directory {path}/made"),
            work.create_directory_at(&format!("{path}/made")),
        );
        say(
            &format!("rename note.txt to {path}/note.txt"),
            work.rename_at("note.txt", work, &format!("{path}/note.txt")),
        );
        say(
            &format!("link note.txt to {path}/linked"),
            work.link_at(
                PathFlags::empty(),
                "note.txt",
                work,
                &format!("{path}/linked"),
            ),
        );
        say(
            &format!("unlink-file {path}/readme.txt"),
            work.unlink_file_at(&format!("{path}/readme.txt")),
        );
    }
    say(
        "rename note.txt into docs' folder",
        work.rename_at("note.txt", docs, "note.txt"),
    );
    say(
        "rename readme.txt out of docs' folder",
        docs.rename_at("readme.txt", work, "readme.txt"),
    );
    for path in ["/mnt/work/../docs/new.txt", "/mnt/work/to-docs/new.txt"] {
        say(
            &format!("std write {path}"),
            fs::write(path, "new").map(|()| "done"),
        );
    }
}

/// Through `std::fs`, from the root: the status of a file and of a symlink to a folder, each named
/// with a `/` after it, and the changes of a folder through each symlink named so.
fn names_ending_in_a_slash_from_the_root() {
    for path in [
        "/data/file.txt/",
        "/data/file.txt/.",
        "/data/link-to-folder/",
    ] {
        let status = fs::symlink_metadata(path).map(|metadata| {
            if metadata.is_dir() {
                "folder"
            } else if metadata.is_symlink() {
                "symlink"
            } else {
                "file"
            }
        });
        say(&format!("std status {path}"), status);
    }
    for (link, _) in LINKS {
        let named = format!("/data/{link}/");
        say(
            &format!("std rename from {named}"),
            fs::rename(&named, "/data/moved"),
        );
        say(
            &format!("std create a folder {named}"),
            fs::create_dir(&named),
        );
        say(
            &format!("std remove the folder {named}"),
            fs::remove_dir(&named),
        );
        say(
            &format!("std remove the file {named}"),
            fs::remove_file(&named),
        );
    }
}

/// What a status says a path names.
fn kind(type_: DescriptorType) -> &'static str {
    match type_ {
        DescriptorType::Directory => "folder",
        DescriptorType::RegularFile => "file",
        DescriptorType::SymbolicLink => "symlink",
        _ => "something else",
    }
}

/// Prints `what: result`: a value, `done` for none, or an error by its name.
fn say<T: Said, E: Error>(what: &str, result: Result<T, E>) {
    match result {
        Ok(value) => println!("{what}: {}", value.said()),
        Err(error) => println!("{what}: {}", error.name()),
    }
}

/// What a call answered, as printed.
trait Said {
    fn said(&self) -> String;
}

impl Said for () {
    fn said(&self) -> String {
        "done".to_owned()
    }
}

impl Said for &str {
    fn said(&self) -> String {
        (*self).to_owned()
    }
}

/// An error, by its name: WASI's for its calls, the kind for `std`'s.
trait Error {
    fn name(&self) -> String;
}

impl Error for ErrorCode {
    fn name(&self) -> String {
        ErrorCode::name(self).to_owned()
    }
}

impl Error for std::io::Error {
    fn name(&self) -> String {
        format!("{:?}", self.kind())
    }
}
