//! The list of notes and files, as the items of the window's `NavigationView`: under the search
//! field, New Note among the notes, then the notes the search finds in the folder, each by its
//! title and, beneath it, when it changed and its line after the title, the one shown selected; a
//! line where it finds none; among the notes, a section of the files open the search finds, each
//! by its name and, beneath it, its folder, marked while it waits for the user; and at its foot,
//! among the notes, Recently Deleted while it holds any, with how many. Each item is known by an id
//! of its own, which names the note or the file it shows.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use waft_foundation::App;
use waft_widgets::{
    BuildContext, Column, CrossAxisAlignment, IntoWidget, MainAxisSize, StatelessWidget, Text,
    TextOverflow, WidgetRef,
};
use waft_winui::{
    InfoBadge, InfoBadgeStyle, NavigationViewItem, NavigationViewItemEventArgs,
    NavigationViewSelectionChangedEventArgs, TextBox,
};

use crate::browsing::{Folder, Shown};
use crate::dates;
use crate::editing::new_note;
use crate::icons;
use crate::note::Note;
use crate::notes_app::NotesApp;
use crate::open_file::OpenFile;
use crate::secondary_text::secondary_text;
use crate::showing::{show, show_folder};

/// The ids of the items that are no note's.
const NEW_NOTE: &str = "new-note";
const NEW_NOTE_LINE: &str = "new-note-line";
const NOTHING_FOUND: &str = "nothing-found";
const FILES_LINE: &str = "files-line";
const FILES: &str = "files";
const RECENTLY_DELETED: &str = "recently-deleted";

/// What the id of an item showing a note begins with, before the note's id, and of one showing a
/// file, before its path.
const NOTE_PREFIX: &str = "note:";
const FILE_PREFIX: &str = "file:";

/// The items of the folder listed: New Note among the notes, the notes the search finds, or a line
/// saying there are none, and the files open it finds, among the notes.
pub(crate) fn menu_items(app: &App, notes: &NotesApp) -> Vec<NavigationViewItem> {
    let browsing = notes.browsing.read(app);
    let mut items = Vec::new();
    if browsing.folder() == Folder::Notes {
        items.push(new_note_item());
        items.push(NavigationViewItem::separator(NEW_NOTE_LINE));
    }
    let now = SystemTime::now();
    let listed = browsing.listed(notes.library.read(app));
    if listed.is_empty() {
        items.push(nothing_found(!browsing.searched().trim().is_empty()));
    }
    items.extend(listed.into_iter().map(|note| note_item(note, now)));
    let files = browsing.listed_files(notes.files.read(app));
    if !files.is_empty() {
        items.push(NavigationViewItem::separator(FILES_LINE));
        items.push(NavigationViewItem::header(FILES, "Files"));
        items.extend(files.into_iter().map(file_item));
    }
    items
}

/// The items at the list's foot: the way to Recently Deleted, among the notes while it holds any.
pub(crate) fn footer_items(app: &App, notes: &NotesApp) -> Vec<NavigationViewItem> {
    let deleted = notes.library.read(app).deleted_count();
    if notes.browsing.read(app).folder() != Folder::Notes || deleted == 0 {
        return Vec::new();
    }
    let count = i32::try_from(deleted).unwrap_or(i32::MAX);
    let item = NavigationViewItem::text(RECENTLY_DELETED, Folder::RecentlyDeleted.name())
        .icon(icons::delete())
        .info_badge(InfoBadge::new().value(count))
        .selects_on_invoked(false);
    vec![item]
}

/// The id of the item showing `shown`.
pub(crate) fn item_of(shown: &Shown) -> String {
    match shown {
        Shown::Note(id) => format!("{NOTE_PREFIX}{id}"),
        Shown::File(path) => format!("{FILE_PREFIX}{}", path.display()),
    }
}

/// What the item `item` shows; none for an item that shows nothing.
fn shown_by(item: &str) -> Option<Shown> {
    if let Some(id) = item.strip_prefix(NOTE_PREFIX) {
        return id.parse().ok().map(Shown::Note);
    }
    let path = item.strip_prefix(FILE_PREFIX)?;
    Some(Shown::File(PathBuf::from(path)))
}

/// The user picked an item: what it shows is shown. A selection cleared, as the navigation clears
/// it when the item selected leaves the list, changes nothing: `showing` shows another.
pub(crate) fn picked(
    app: &mut App,
    notes: &NotesApp,
    args: NavigationViewSelectionChangedEventArgs,
) {
    if let Some(shown) = args.item.and_then(|item| shown_by(&item.id)) {
        show(app, notes, Some(shown));
    }
}

/// The user invoked an item that shows no note: New Note, or Recently Deleted.
pub(crate) fn invoked(app: &mut App, notes: &NotesApp, args: NavigationViewItemEventArgs) {
    match args.item.id.as_str() {
        NEW_NOTE => new_note(app, notes),
        RECENTLY_DELETED => show_folder(app, notes, Folder::RecentlyDeleted),
        _ => {}
    }
}

/// The search field, with what it searches for.
pub(crate) fn search_field(notes: &NotesApp) -> TextBox {
    TextBox::new(notes.search.text)
        .focus_node(notes.search.focus)
        .placeholder_text("Search")
}

fn new_note_item() -> NavigationViewItem {
    NavigationViewItem::text(NEW_NOTE, "New Note")
        .icon(icons::compose())
        .selects_on_invoked(false)
}

/// The line a list with no notes shows: `No Results` while `searching`, `No Notes` otherwise.
fn nothing_found(searching: bool) -> NavigationViewItem {
    let said = if searching { "No Results" } else { "No Notes" };
    NavigationViewItem::text(NOTHING_FOUND, said)
        .is_enabled(false)
        .selects_on_invoked(false)
}

/// The item of `note`, by its title and, beneath it, when it changed and its line after the title.
fn note_item(note: &Note, now: SystemTime) -> NavigationViewItem {
    let changed = dates::listed(note.changed, now, dates::users_offset);
    let detail = match note.preview() {
        "" => changed,
        preview => format!("{changed}  {preview}"),
    };
    let title = note.title().to_owned();
    let mut item = NavigationViewItem::new(
        item_of(&Shown::Note(note.id)),
        TwoLines {
            title: title.clone(),
            detail,
        },
    )
    .icon(icons::note());
    // The tooltip the item shows while the list is only its icons.
    item.text = Some(title);
    item
}

/// The item of `file`, by its name and, beneath it, its folder; marked while it waits for the
/// user, as another app changed or removed it, or it could not be written.
fn file_item(file: &OpenFile) -> NavigationViewItem {
    let name = file.name();
    let folder = file.path().parent().map(Path::display);
    let mut item = NavigationViewItem::new(
        item_of(&Shown::File(file.path().to_owned())),
        TwoLines {
            title: name.clone(),
            detail: folder.map(|folder| folder.to_string()).unwrap_or_default(),
        },
    )
    .icon(icons::document());
    if file.problem().is_some() {
        item = item.info_badge(InfoBadge::new().style(InfoBadgeStyle::Caution));
    }
    item.text = Some(name);
    item
}

/// A title on one line, and a line said aside beneath it.
#[derive(Debug)]
struct TwoLines {
    title: String,
    detail: String,
}

impl StatelessWidget for TwoLines {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let title = Text::new(self.title.clone())
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis);
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([
                title.into_widget(),
                secondary_text(app, context, self.detail.clone()).into_widget(),
            ])
            .into_widget()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::NoteId;

    #[test]
    fn a_notes_item_names_it_by_its_id() {
        let shopping = Shown::Note(NoteId(42));
        assert_eq!(item_of(&shopping), "note:42");
        assert_eq!(shown_by(&item_of(&shopping)), Some(shopping));
    }

    #[test]
    fn a_files_item_names_it_by_its_path() {
        let todo = Shown::File(PathBuf::from("/Users/me/to do: 1.txt"));
        assert_eq!(item_of(&todo), "file:/Users/me/to do: 1.txt");
        assert_eq!(shown_by(&item_of(&todo)), Some(todo));
    }

    #[test]
    fn an_item_of_nothing_shown_names_none() {
        for item in [NEW_NOTE, NOTHING_FOUND, FILES, RECENTLY_DELETED, "note:"] {
            assert_eq!(shown_by(item), None, "{item}");
        }
    }
}
