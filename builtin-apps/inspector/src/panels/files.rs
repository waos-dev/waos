//! A program's files as it opens and changes them: a row per event, the path as the program named
//! it, what it did, when and how WaOS decided it; beneath, where the path is on this computer, and
//! who it was for where that was not the program itself, as the system's codecs reading an image.
//!
//! The invariant: the path the program named and the place on this computer stay apart, since a
//! program names its own folders (`/data`, a pick) and only WaOS knows where they are.

use wasdk::inspect::trace::{Cause, FileEvent, FileUse};

use super::{Column, Decided, Kept, Panel, PanelRow};
use crate::{clock, words};

const COLUMNS: [Column; 4] = [
    ("Path", 0.0),
    ("Kind", 116.0),
    ("Started", 76.0),
    ("Decided", 72.0),
];

/// The rows heard so far.
#[derive(Debug, Default)]
pub(crate) struct FileLog {
    rows: Kept<FileRow>,
}

impl FileLog {
    pub(crate) fn add(&mut self, event: &FileEvent) {
        self.rows.push(FileRow::of(event));
    }
}

impl Panel for FileLog {
    fn columns(&self) -> &'static [Column] {
        &COLUMNS
    }

    fn rows(&self) -> Vec<PanelRow> {
        self.rows.iter().map(FileRow::drawn).collect()
    }

    fn nothing_yet(&self) -> &'static str {
        words::NO_FILES_YET
    }
}

#[derive(Clone, Debug, PartialEq)]
struct FileRow {
    at: i64,
    kind: String,
    /// The path as the program named it, with where it led for a rename or a link.
    path: String,
    /// Where the path is on this computer; empty where it led nowhere.
    on_machine: String,
    /// Who it was for, where that was not the program itself.
    for_whom: Option<&'static str>,
    decided: Decided,
}

impl FileRow {
    fn of(event: &FileEvent) -> FileRow {
        let named = event
            .path
            .clone()
            .unwrap_or_else(|| words::THROUGH_AN_OPEN_FILE.to_owned());
        let (kind, path) = match &event.kind {
            FileUse::OpenToRead => ("Open to read".to_owned(), named),
            FileUse::OpenToWrite => ("Open to write".to_owned(), named),
            FileUse::CreateDirectory => ("Create folder".to_owned(), named),
            FileUse::Remove => ("Remove".to_owned(), named),
            FileUse::RemoveDirectory => ("Remove folder".to_owned(), named),
            FileUse::Rename(to) => ("Rename".to_owned(), format!("{named} → {to}")),
            FileUse::HardLink(at) => ("Hard link".to_owned(), format!("{at} → {named}")),
            FileUse::Symlink(holding) => ("Symlink".to_owned(), format!("{named} → {holding}")),
            FileUse::SetTimes => ("Set times".to_owned(), named),
            FileUse::MetadataRead => ("Read metadata".to_owned(), named),
            FileUse::FirstWrite => ("First write".to_owned(), named),
            FileUse::FolderWatch(options) if options.recursive => {
                ("Watch, and under it".to_owned(), named)
            }
            FileUse::FolderWatch(_) => ("Watch".to_owned(), named),
        };
        FileRow {
            at: event.at.seconds,
            kind,
            path,
            on_machine: event.on_machine.clone().unwrap_or_default(),
            for_whom: for_whom(event.cause),
            decided: Decided::of(&event.verdict),
        }
    }

    fn drawn(&self) -> PanelRow {
        let (started, decided) = (
            clock::time_of_day(self.at),
            words::decided(&self.decided).to_owned(),
        );
        let mut beneath = Vec::new();
        if !self.on_machine.is_empty() {
            beneath.push(words::on_this_computer(&self.on_machine));
        }
        beneath.extend(self.for_whom.map(str::to_owned));
        beneath.extend(words::why_decided(&self.decided));
        PanelRow {
            cells: vec![
                self.path.clone(),
                self.kind.clone(),
                started.clone(),
                decided.clone(),
            ],
            title: format!("{} {}", self.kind, self.path),
            details: vec![started, decided],
            beneath,
        }
    }
}

/// Who a use was for, where it was not the program itself.
fn for_whom(cause: Cause) -> Option<&'static str> {
    match cause {
        Cause::Program => None,
        Cause::HttpRequest => Some("For one of its HTTP requests"),
        Cause::MediaPlayer => Some("For the system's media player"),
        Cause::Opener => Some("For the app the system opens it in"),
        Cause::ImageCodecs => Some("For the system's image codecs"),
        Cause::FileDialog => Some("For a file dialog"),
        Cause::Clipboard => Some("For what the user pastes it into"),
    }
}
