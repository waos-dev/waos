//! A program's files as it opens and changes them: a row per event, the path as the program named
//! it, what it did, how WaOS decided it and when; in its detail, where the path is on this
//! computer, and who it was for where that was not the program itself, as the system's codecs
//! reading an image.
//!
//! The invariant: the path the program named and the place on this computer stay apart, since a
//! program names its own folders (`/data`, a pick) and only WaOS knows where they are.

use wasdk::inspect::trace::{Cause, FileEvent, FileUse};
use wasdk::system_clock::Instant;

use super::{Cell, Column, Decided, PanelRow, RowKind, Rows, Tile, Tone, found_in};
use crate::clock;
use crate::logic::detail::{self, Detail, Field, Glyph, Refusal, Section};
use crate::words;

/// What the panel hears, row by row.
#[derive(Debug, Default)]
pub(crate) struct FileLog;

impl FileLog {
    pub(crate) fn add(&mut self, rows: &mut impl Rows<FileRow>, event: &FileEvent) {
        rows.append(FileRow::of(event));
    }
}

/// The kinds of a file event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Open,
    Change,
    Look,
    Watch,
}

impl RowKind for FileKind {
    const ALL: &'static [FileKind] = &[
        FileKind::Open,
        FileKind::Change,
        FileKind::Look,
        FileKind::Watch,
    ];

    fn name(self) -> &'static str {
        match self {
            FileKind::Open => "Opens",
            FileKind::Change => "Changes",
            FileKind::Look => "Metadata",
            FileKind::Watch => "Watches",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FileRow {
    at: Instant,
    instance: u32,
    kind: FileKind,
    what: &'static str,
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
        let (kind, what, path) = match &event.kind {
            FileUse::OpenToRead => (FileKind::Open, "Open to read", named),
            FileUse::OpenToWrite => (FileKind::Open, "Open to write", named),
            FileUse::CreateDirectory => (FileKind::Change, "Create folder", named),
            FileUse::Remove => (FileKind::Change, "Remove", named),
            FileUse::RemoveDirectory => (FileKind::Change, "Remove folder", named),
            FileUse::Rename(to) => (FileKind::Change, "Rename", format!("{named} → {to}")),
            FileUse::HardLink(at) => (FileKind::Change, "Hard link", format!("{at} → {named}")),
            FileUse::Symlink(holding) => {
                (FileKind::Change, "Symlink", format!("{named} → {holding}"))
            }
            FileUse::SetTimes => (FileKind::Change, "Set times", named),
            FileUse::FirstWrite => (FileKind::Change, "First write", named),
            FileUse::MetadataRead => (FileKind::Look, "Read metadata", named),
            FileUse::FolderWatch(options) if options.recursive => {
                (FileKind::Watch, "Watch, and under it", named)
            }
            FileUse::FolderWatch(_) => (FileKind::Watch, "Watch", named),
        };
        FileRow {
            at: event.at,
            instance: event.instance,
            kind,
            what,
            path,
            on_machine: event.on_machine.clone().unwrap_or_default(),
            for_whom: for_whom(event.cause),
            decided: Decided::of(&event.verdict),
        }
    }
}

impl PanelRow for FileRow {
    type Kind = FileKind;

    const COLUMNS: &'static [Column] = &[
        Column::rest("Path"),
        Column::points("Kind", 132.0).stepping_out(),
        Column::points("Decided", 150.0),
        Column::numeric("Started", 130.0),
    ];

    const NOTHING_YET: &'static str = words::NO_FILES_YET;

    fn kind(&self) -> FileKind {
        self.kind
    }

    fn glyph(&self) -> Glyph {
        match self.kind {
            FileKind::Watch => Glyph::Watch,
            _ if self.what.ends_with("folder") => Glyph::Folder,
            _ => Glyph::File,
        }
    }

    fn cell(&self, index: usize) -> Cell {
        match index {
            0 => {
                let path = Cell::code(self.path.clone());
                if self.decided.refused() {
                    path.toned(Tone::Refused)
                } else {
                    path
                }
            }
            1 => Cell::plain(self.what).toned(Tone::Quiet),
            2 => self.decided.cell(),
            _ => Cell::code(clock::precise(self.at)).toned(Tone::Quiet),
        }
    }

    fn refused(&self) -> bool {
        self.decided.refused()
    }

    fn matches(&self, text: &str) -> bool {
        found_in(text, &[&self.path, &self.on_machine, self.what])
    }

    fn tile(&self) -> Tile {
        let mut beneath = Vec::new();
        if !self.on_machine.is_empty() {
            beneath.push(words::on_this_computer(&self.on_machine));
        }
        beneath.extend(self.for_whom.map(str::to_owned));
        beneath.extend(words::why_decided(&self.decided));
        Tile {
            code: true,
            title: format!("{} {}", self.what, self.path),
            details: vec![
                clock::time_of_day(self.at.seconds),
                words::decided(&self.decided).to_owned(),
            ],
            beneath,
        }
    }

    fn detail(&self) -> Detail {
        let mut fields = vec![
            Field::plain(words::KIND, self.what),
            Field::code(words::ON_THIS_COMPUTER, &self.on_machine),
            Field::plain(words::INSTANCE, words::instance(self.instance)),
            Field::code(words::AT, clock::precise(self.at)),
        ];
        if self.on_machine.is_empty() {
            fields.remove(1);
        }
        if let Some(for_whom) = self.for_whom {
            fields.push(Field::plain(words::FOR, for_whom));
        }
        Detail {
            glyph: self.glyph(),
            title: self.what.to_owned(),
            subtitle: self.path.clone(),
            refusal: Refusal::of(&self.decided),
            sections: vec![
                Section {
                    title: words::EVENT,
                    fields,
                },
                detail::decision(&self.decided),
            ],
        }
    }
}

/// Who a use was for, where it was not the program itself.
fn for_whom(cause: Cause) -> Option<&'static str> {
    match cause {
        Cause::Program => None,
        Cause::HttpRequest => Some("One of its HTTP requests"),
        Cause::MediaPlayer => Some("The system's media player"),
        Cause::Opener => Some("The app the system opens it in"),
        Cause::ImageCodecs => Some("The system's image codecs"),
        Cause::FileDialog => Some("A file dialog"),
        Cause::Clipboard => Some("What the user pastes it into"),
    }
}
