//! The window's page: a Fluent `NavigationView` on a `Page`, clear of a phone's bars and keyboard, its pane the list of feeds (`feed_list`), its
//! content what the window shows (`content`) under a header that names it and holds its commands
//! (`header`). From 640 logical points wide the pane is a sidebar beside the content, as on a
//! desktop or a tablet; narrower, as on a phone, it is a pane over the content that its toggle
//! opens and that closes as a feed is picked. The navigation's Back goes from an item being read,
//! or the settings, to the items listed.
//!
//! The pane's openness is the app's (`Browsing`), as the navigation reports it, so that a command
//! can close it over what it shows.

use std::rc::Rc;

use waft::{App, BuildContext, IntoWidget, Listener, StatelessWidget, WidgetRef};
use waft_winui::{
    Brush, NavigationView, NavigationViewBackButtonVisible, NavigationViewDisplayMode,
    ThemeResources,
};

use crate::browsing::Page;
use crate::content::Content;
use crate::feed_list::{footer_items, invoked, menu_items, picked, selected_item};
use crate::header::Header;
use crate::reader_app::ReaderApp;
use crate::showing::back_to_items;

/// The width from which the list of feeds is a sidebar, in logical points.
const SIDEBAR_FROM_WIDTH: f64 = 640.0;

/// The sidebar's width, in logical points.
const SIDEBAR_WIDTH: f64 = 250.0;

#[derive(Debug)]
pub(crate) struct MainPage {
    pub(crate) reader: ReaderApp,
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
        let reader = &self.reader;
        let browsing = reader.browsing.read(app);
        let (picking, invoking) = (reader.clone(), reader.clone());
        let navigation = NavigationView::new(
            menu_items(app, reader),
            Some(selected_item(browsing.scope(), browsing.page())),
            move |app, args| picked(app, &picking, args),
            Content {
                reader: reader.clone(),
            },
        )
        .item_invoked(move |app, args| invoked(app, &invoking, args))
        .footer_menu_items(footer_items())
        .pane_title("Reader")
        .header(Header {
            reader: reader.clone(),
        })
        .compact_mode_threshold_width(SIDEBAR_FROM_WIDTH)
        .expanded_mode_threshold_width(SIDEBAR_FROM_WIDTH)
        .open_pane_length(SIDEBAR_WIDTH);
        let navigation = following_the_pane(navigation, reader, browsing.pane_open());
        with_back_to_items(navigation, reader, browsing.page(), browsing.narrow())
    }
}

/// `navigation` on the window's page, which reaches under a phone's bars: the navigation's pane
/// and content run to the screen's edges and keep their controls clear of the bars.
fn on_page(resources: &ThemeResources, navigation: NavigationView) -> WidgetRef {
    waft_winui::Page::new()
        .background(Brush::Solid(resources.solid_background_fill_color_base))
        .extends_content_into_system_bars(true)
        .content(navigation)
        .into_widget()
}

/// `navigation`, its pane `open` as the app keeps it, and the app told how the navigation lays the
/// pane out and when it opens or closes it.
fn following_the_pane(
    navigation: NavigationView,
    reader: &ReaderApp,
    open: bool,
) -> NavigationView {
    let toggling = reader.clone();
    let mut navigation =
        navigation.is_pane_open(open, move |app, open| pane_toggled(app, &toggling, open));
    let laying_out = reader.clone();
    navigation.display_mode_changed = Some(Rc::new(move |app, mode| {
        laid_out(app, &laying_out, mode);
    }));
    navigation
}

/// `navigation`, with Back to the items listed, which the user may take while an item is read or
/// the settings are shown. Beside a sidebar it keeps its place above the pane meanwhile, so that
/// opening an item moves nothing; at a phone's width it shows only while it may be taken, and
/// leaves its room to the header's title otherwise.
fn with_back_to_items(
    navigation: NavigationView,
    reader: &ReaderApp,
    page: &Page,
    narrow: bool,
) -> NavigationView {
    let may_go_back = *page != Page::Items;
    let visible = if may_go_back || !narrow {
        NavigationViewBackButtonVisible::Visible
    } else {
        NavigationViewBackButtonVisible::Collapsed
    };
    let mut navigation = navigation
        .is_back_button_visible(visible)
        .is_back_enabled(may_go_back);
    let reader = reader.clone();
    navigation.back_requested = Some(Listener::new(move |app: &mut App| {
        back_to_items(app, &reader);
    }));
    navigation
}

/// The navigation opened or closed its pane, as the user asked or as its layout changed.
fn pane_toggled(app: &mut App, reader: &ReaderApp, open: bool) {
    reader.browsing.update(app, |browsing, cx| {
        browsing.set_pane_open(open);
        cx.notify();
    });
}

/// The navigation took the layout `mode` for the window's width: a pane over the content where it
/// is minimal, a sidebar otherwise.
fn laid_out(app: &mut App, reader: &ReaderApp, mode: NavigationViewDisplayMode) {
    reader.browsing.update(app, |browsing, cx| {
        browsing.set_narrow(mode == NavigationViewDisplayMode::Minimal);
        cx.notify();
    });
}
