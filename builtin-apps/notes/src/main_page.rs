//! The window's page: a Fluent `NavigationView` on a `FluentPage`, clear of a phone's bars and keyboard, its pane the list (`note_list`) under the folder's
//! name and the search field, its content the note or the file shown (`editor_pane`). From 600 logical points
//! wide the pane is a sidebar beside the note, as on a desktop or a tablet; narrower, as on a phone
//! or in a narrow window, it is a pane over the note that its toggle opens and that closes as a note
//! is picked. The line is Material 3's between its compact window class and the rest, where WaOS's
//! library changes its layout too.
//!
//! The pane's openness is the app's (`Browsing`), as the navigation reports it, so that a command
//! can open it, as Search does, or close it over the note it opens. A pane opening over the note
//! takes the focus, so Search moves it to the search field once the pane has opened.

use std::rc::Rc;

use waft_foundation::{App, Listener};
use waft_widgets::{BuildContext, IntoWidget, StatelessWidget, WidgetRef};
use waft_winui::{
    Brush, FluentPage, NavigationView, NavigationViewBackButtonVisible, NavigationViewDisplayMode,
    ThemeResources,
};

use crate::browsing::Folder;
use crate::editor_pane::{EditorHeader, EditorPane};
use crate::list_pane::pane_has_opened;
use crate::note_list::{footer_items, invoked, item_of, menu_items, picked, search_field};
use crate::notes_app::NotesApp;
use crate::showing::show_folder;

/// The width from which the list is a sidebar, in logical points.
const SIDEBAR_FROM_WIDTH: f64 = 600.0;

/// The sidebar's width, in logical points.
const SIDEBAR_WIDTH: f64 = 260.0;

#[derive(Debug)]
pub(crate) struct MainPage {
    pub(crate) notes: NotesApp,
}

impl StatelessWidget for MainPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let navigation = self.navigation(app);
        on_page(&resources, navigation)
    }
}

impl MainPage {
    fn navigation(&self, app: &App) -> NavigationView {
        let notes = &self.notes;
        let browsing = notes.browsing.read(app);
        let folder = browsing.folder();
        let (picking, invoking) = (notes.clone(), notes.clone());
        let navigation = NavigationView::new(
            menu_items(app, notes),
            browsing.shown().map(item_of),
            move |app, args| picked(app, &picking, args),
            EditorPane {
                notes: notes.clone(),
            },
        )
        .item_invoked(move |app, args| invoked(app, &invoking, args))
        .footer_menu_items(footer_items(app, notes))
        .pane_title(folder.name())
        .auto_suggest_box(search_field(notes))
        .header(EditorHeader {
            notes: notes.clone(),
        })
        .is_settings_visible(false)
        .compact_mode_threshold_width(SIDEBAR_FROM_WIDTH)
        .expanded_mode_threshold_width(SIDEBAR_FROM_WIDTH)
        .open_pane_length(SIDEBAR_WIDTH);
        let navigation = following_the_pane(navigation, notes, browsing.pane_open());
        with_back_to_notes(navigation, notes, folder)
    }
}

/// `navigation` on the window's page, which reaches under a phone's bars: the navigation's pane
/// and content run to the screen's edges and keep their controls clear of the bars.
fn on_page(resources: &ThemeResources, navigation: NavigationView) -> WidgetRef {
    FluentPage::new()
        .background(Brush::Solid(resources.solid_background_fill_color_base))
        .extends_content_into_system_bars(true)
        .content(navigation)
        .into_widget()
}

/// `navigation`, its pane `open` as the app keeps it, and the app told how the navigation lays the
/// pane out, when it opens or closes it, and when it has opened.
fn following_the_pane(navigation: NavigationView, notes: &NotesApp, open: bool) -> NavigationView {
    let toggling = notes.clone();
    let mut navigation =
        navigation.is_pane_open(open, move |app, open| pane_toggled(app, &toggling, open));
    let laying_out = notes.clone();
    navigation.display_mode_changed = Some(Rc::new(move |app, mode| {
        laid_out(app, &laying_out, mode);
    }));
    let opening = notes.clone();
    navigation.pane_opened = Some(Listener::new(move |app| pane_has_opened(app, &opening)));
    navigation
}

/// `navigation`, with Back to the notes while it lists Recently Deleted.
fn with_back_to_notes(
    navigation: NavigationView,
    notes: &NotesApp,
    folder: Folder,
) -> NavigationView {
    if folder == Folder::Notes {
        return navigation.is_back_button_visible(NavigationViewBackButtonVisible::Collapsed);
    }
    let mut navigation = navigation
        .is_back_button_visible(NavigationViewBackButtonVisible::Visible)
        .is_back_enabled(true);
    let notes = notes.clone();
    navigation.back_requested = Some(Listener::new(move |app| {
        show_folder(app, &notes, Folder::Notes);
    }));
    navigation
}

/// The navigation opened or closed its pane, as the user asked or as its layout changed.
fn pane_toggled(app: &mut App, notes: &NotesApp, open: bool) {
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_pane_open(open);
        cx.notify();
    });
}

/// The navigation took the layout `mode` for the window's width: a pane over the note where it
/// is minimal, a sidebar otherwise.
fn laid_out(app: &mut App, notes: &NotesApp, mode: NavigationViewDisplayMode) {
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_narrow(mode == NavigationViewDisplayMode::Minimal);
        cx.notify();
    });
}
