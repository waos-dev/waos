//! The library's page, in Fluent's `NavigationView`: its sections in the pane, beside the grid
//! where the window is wide and along its top where it is as narrow as a phone, All, Continue
//! Watching, and each folder under Folders, with Add Folder… at the pane's foot; the search in the
//! pane's search box; a header with the section's name and its commands, opening a file or a link
//! and, for a folder, taking it out of the library; and the grid of what the section shows, as
//! many columns as the width holds, or a line on what to do while it shows nothing.

use std::fmt;

use waft_foundation::{App, Entity, Listener, Ref};
use waft_widgets::{
    BuildContext, Center, Column, EdgeInsetsGeometry, Expanded, GridView, IntoWidget,
    LayoutBuilder, MainAxisSize, Padding, Row, SizedBox, StatelessWidget, TextEditingController,
    TextEditingControllerLeaf, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, NavigationView, NavigationViewBackButtonVisible, NavigationViewItem,
    NavigationViewItemEventArgs, NavigationViewPaneDisplayMode,
    NavigationViewSelectionChangedEventArgs, TextBlockStyle, TextBox,
};

use crate::folders::name_of;
use crate::icons::{self, icon};
use crate::library::{self, Library, Tile};
use crate::opening;
use crate::playback::Playback;
use crate::sections::Section;
use crate::text::{Tone, line, text};
use crate::video_tile::{CAPTION_HEIGHT, THUMBNAIL_ASPECT, video_tile};

/// How wide the window is, at the least, for the sections in a pane beside the grid.
const WIDE: f64 = 640.0;

/// How wide the pane is, beside the grid.
const PANE_WIDTH: f64 = 240.0;

/// The narrowest a tile is made: the grid has a column for each this wide that fits.
const NARROWEST_TILE: f64 = 160.0;

/// The room between tiles, and around the grid.
const SPACING: f64 = 16.0;

/// The ids of the pane's items that are not a folder's.
const ALL: &str = "all";
const CONTINUE_WATCHING: &str = "continue-watching";
const FOLDERS: &str = "folders";
const ADD_FOLDER: &str = "add-folder";

/// What a folder's item id starts with, before its place among the folders.
const FOLDER: &str = "folder:";

/// The library's page, its tiles watched as far as `playback`'s recent list says, each played
/// there.
pub(crate) struct LibraryScreen {
    library: Entity<Library>,
    playback: Entity<Playback>,
}

impl LibraryScreen {
    pub(crate) fn new(library: Entity<Library>, playback: Entity<Playback>) -> LibraryScreen {
        LibraryScreen { library, playback }
    }
}

impl fmt::Debug for LibraryScreen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryScreen").finish_non_exhaustive()
    }
}

/// What the page shows, read as it builds.
#[derive(Clone)]
struct Shown {
    library: Entity<Library>,
    playback: Entity<Playback>,
    search_field: Ref<TextEditingController>,
    folders: Vec<String>,
    section: Section,
    search: String,
    tiles: Vec<Tile>,
}

impl StatelessWidget for LibraryScreen {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let library = self.library.read(app);
        let search = library.search_field().text_value(app).to_owned();
        let shown = Shown {
            library: self.library.clone(),
            playback: self.playback.clone(),
            search_field: library.search_field().clone(),
            folders: library
                .folders()
                .iter()
                .map(|folder| name_of(folder))
                .collect(),
            section: library.section(),
            tiles: library.tiles(&search, self.playback.read(app).recents()),
            search,
        };
        LayoutBuilder::new(move |_app, context, constraints| {
            let pane = if constraints.max_width >= WIDE {
                NavigationViewPaneDisplayMode::Left
            } else {
                NavigationViewPaneDisplayMode::Top
            };
            navigation(&shown, pane, context)
        })
        .into_widget()
    }
}

/// The sections in a pane shown as `pane` says, over the section's header and videos.
fn navigation(
    shown: &Shown,
    pane: NavigationViewPaneDisplayMode,
    context: BuildContext,
) -> WidgetRef {
    let chosen = {
        let library = shown.library.clone();
        move |app: &mut App, args: NavigationViewSelectionChangedEventArgs| {
            if let Some(section) = args.item.and_then(|item| section_of(&item.id)) {
                library::choose(&library, app, section);
            }
        }
    };
    let invoked = {
        let library = shown.library.clone();
        move |app: &mut App, args: NavigationViewItemEventArgs| {
            if args.item.id == ADD_FOLDER {
                opening::add_folder(&library, app);
            }
        }
    };
    let add_folder = NavigationViewItem::text(ADD_FOLDER, "Add Folder…")
        .icon(icon(icons::FOLDER_ADD, 16.0))
        .selects_on_invoked(false);
    let content = Column::new().children([
        header(shown, context),
        Expanded::new(videos(shown)).into_widget(),
    ]);
    NavigationView::new(
        section_items(shown),
        Some(id_of(shown.section)),
        chosen,
        content,
    )
    .pane_display_mode(pane)
    .open_pane_length(PANE_WIDTH)
    .pane_title("Library")
    .is_settings_visible(false)
    .is_pane_toggle_button_visible(false)
    .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
    .auto_suggest_box(search_box(shown))
    .footer_menu_items([add_folder])
    .item_invoked(invoked)
    .into_widget()
}

/// All, Continue Watching, and each folder under Folders.
fn section_items(shown: &Shown) -> Vec<NavigationViewItem> {
    let mut items = vec![
        NavigationViewItem::text(ALL, "All").icon(icon(icons::LIBRARY, 16.0)),
        NavigationViewItem::text(CONTINUE_WATCHING, "Continue Watching")
            .icon(icon(icons::HISTORY, 16.0)),
    ];
    if !shown.folders.is_empty() {
        items.push(NavigationViewItem::header(FOLDERS, "Folders"));
    }
    items.extend(shown.folders.iter().enumerate().map(|(index, name)| {
        NavigationViewItem::text(id_of(Section::Folder(index)), name.clone())
            .icon(icon(icons::FOLDER, 16.0))
    }));
    items
}

/// The pane item's id of `section`.
fn id_of(section: Section) -> String {
    match section {
        Section::All => ALL.to_owned(),
        Section::ContinueWatching => CONTINUE_WATCHING.to_owned(),
        Section::Folder(index) => format!("{FOLDER}{index}"),
    }
}

/// The section the pane item `id` names; none for an item of another kind.
fn section_of(id: &str) -> Option<Section> {
    match id {
        ALL => Some(Section::All),
        CONTINUE_WATCHING => Some(Section::ContinueWatching),
        _ => id.strip_prefix(FOLDER)?.parse().ok().map(Section::Folder),
    }
}

/// The search, by title, whose text the library keeps.
fn search_box(shown: &Shown) -> WidgetRef {
    let library = shown.library.clone();
    TextBox::new(shown.search_field.erase())
        .placeholder_text("Search")
        .text_changed(move |app: &mut App, _text: String| library::search_changed(&library, app))
        .into_widget()
}

/// The section's name, and its commands: opening a file or a link, and taking a folder out of the
/// library.
fn header(shown: &Shown, context: BuildContext) -> WidgetRef {
    let name = match shown.section {
        Section::All => "All".to_owned(),
        Section::ContinueWatching => "Continue Watching".to_owned(),
        Section::Folder(index) => shown.folders.get(index).cloned().unwrap_or_default(),
    };
    let mut row =
        vec![Expanded::new(line(name, TextBlockStyle::Subtitle, Tone::Primary)).into_widget()];
    if let Section::Folder(index) = shown.section {
        let library = shown.library.clone();
        let remove =
            Listener::new(move |app: &mut App| library::remove_folder(&library, app, index));
        row.push(command(icons::DELETE, "Remove Folder", remove));
    }
    let open_file = {
        let playback = shown.playback.clone();
        Listener::new(move |app: &mut App| opening::open_file(&playback, app))
    };
    let open_location = {
        let playback = shown.playback.clone();
        Listener::new(move |app: &mut App| opening::open_location(&playback, app, context))
    };
    row.push(command(icons::DOCUMENT, "Open…", open_file));
    row.push(command(icons::LINK, "Open Location…", open_location));
    Padding::new(EdgeInsetsGeometry::only(SPACING, SPACING, SPACING, 0.0))
        .child(Row::new().spacing(8.0).children(row))
        .into_widget()
}

/// A subtle button of `glyph` and `label`, which does `pressed`.
fn command(glyph: char, label: &str, pressed: Listener) -> WidgetRef {
    let face = Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(8.0)
        .children([
            icon(glyph, 16.0).into_widget(),
            line(label, TextBlockStyle::Body, Tone::Primary),
        ]);
    Button::new(face, pressed)
        .style(ButtonStyle::Subtle)
        .into_widget()
}

/// The grid of the tiles shown, or a line on why there are none.
fn videos(shown: &Shown) -> WidgetRef {
    if shown.tiles.is_empty() {
        return nothing_shown(shown);
    }
    let (playback, tiles) = (shown.playback.clone(), shown.tiles.clone());
    LayoutBuilder::new(move |_app, _context, constraints| {
        let room = (constraints.max_width - 2.0 * SPACING).max(NARROWEST_TILE);
        let columns = columns_for(room);
        let tile_width = (room - SPACING * (columns - 1) as f64) / columns as f64;
        let tiles = tiles
            .iter()
            .cloned()
            .map(|tile| video_tile(&playback, tile));
        GridView::count(columns as i32)
            .main_axis_spacing(SPACING)
            .cross_axis_spacing(SPACING)
            .main_axis_extent(tile_width / THUMBNAIL_ASPECT + CAPTION_HEIGHT)
            .children(tiles.collect::<Vec<_>>())
            .build()
            .padding(EdgeInsetsGeometry::all(SPACING))
            .into_widget()
    })
    .into_widget()
}

/// How many columns of tiles `room` holds, each at least [`NARROWEST_TILE`] wide: one at the
/// least.
fn columns_for(room: f64) -> usize {
    (((room + SPACING) / (NARROWEST_TILE + SPACING)).floor() as usize).max(1)
}

/// Why the section shows nothing, and, for an empty library, what adds to it.
fn nothing_shown(shown: &Shown) -> WidgetRef {
    if shown.folders.is_empty() {
        let library = shown.library.clone();
        let add = Listener::new(move |app: &mut App| opening::add_folder(&library, app));
        return centred(vec![
            text(
                "Add a folder of videos to see them here.",
                TextBlockStyle::BodyLarge,
                Tone::Primary,
            ),
            SizedBox::new().height(16.0).into_widget(),
            Button::text("Add Folder…", add)
                .style(ButtonStyle::Accent)
                .into_widget(),
        ]);
    }
    let why = if !shown.search.trim().is_empty() {
        format!("No videos match “{}”.", shown.search.trim())
    } else {
        match shown.section {
            Section::All => "No videos in these folders.".to_owned(),
            Section::Folder(_) => "No videos in this folder.".to_owned(),
            Section::ContinueWatching => "Videos you leave partway show here.".to_owned(),
        }
    };
    centred(vec![text(why, TextBlockStyle::BodyLarge, Tone::Secondary)])
}

fn centred(children: Vec<WidgetRef>) -> WidgetRef {
    Center::new()
        .child(
            Column::new()
                .main_axis_size(MainAxisSize::Min)
                .children(children),
        )
        .into_widget()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_has_as_many_columns_as_its_width_holds() {
        assert_eq!(columns_for(100.0), 1, "one at the least");
        assert_eq!(columns_for(NARROWEST_TILE), 1);
        assert_eq!(columns_for(2.0 * NARROWEST_TILE + SPACING), 2);
        assert_eq!(columns_for(548.0), 3);
        assert_eq!(columns_for(1200.0), 6);
    }

    #[test]
    fn a_sections_item_names_it_back() {
        for section in [Section::All, Section::ContinueWatching, Section::Folder(3)] {
            assert_eq!(section_of(&id_of(section)), Some(section));
        }
        assert_eq!(section_of(ADD_FOLDER), None);
        assert_eq!(section_of("folder:x"), None);
    }
}
