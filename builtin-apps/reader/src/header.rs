//! The header above the content: what is shown, by name, and its commands. Above the items, the
//! name of what is listed, with Mark All as Read and Refresh, which turns while a refresh is
//! under way; above an item being read, its feed's name, said aside, since the item's own title
//! is the page's, with Mark as Unread and Open in Browser; above the settings, their name.

use waft::{
    App, BuildContext, Center, EdgeInsetsGeometry, Expanded, IntoWidget, Padding, Row, SizedBox,
    StatelessWidget, Text, TextOverflow, WidgetRef,
};
use waft_winui::{ProgressRing, TextBlockStyle, ThemeResources};

use crate::bar_button::icon_button;
use crate::browsing::Page;
use crate::icons;
use crate::library::Scope;
use crate::opening_links::open_original;
use crate::reader_app::ReaderApp;
use crate::refreshing::{Asked, refresh};
use crate::showing::{keep_unread, mark_listed_read};

/// The space right of the commands, to the window's edge, in logical points.
const INSET: f64 = 12.0;

/// The size of the ring that turns in Refresh's place while a refresh is under way, and the room
/// Refresh's place takes with either in it, so that the header stays as it is as they change.
const RING_SIZE: f64 = 16.0;
const REFRESH_ROOM: (f64, f64) = (40.0, 36.0);

#[derive(Debug)]
pub(crate) struct Header {
    pub(crate) reader: ReaderApp,
}

impl StatelessWidget for Header {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let mut title = Text::new(self.title(app))
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis);
        if self.reader.browsing.read(app).reading().is_some() {
            let resources = ThemeResources::of(app, context);
            let aside = resources.text_fill_color_secondary;
            title = title.style(TextBlockStyle::BodyStrong.text_style(&resources, aside));
        }
        let mut parts = vec![Expanded::new(title).into_widget()];
        parts.extend(self.commands(app));
        Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 0.0, INSET, 0.0))
            .child(Row::new().children(parts))
            .into_widget()
    }
}

impl Header {
    /// The name of what is shown: of what is listed, of the feed whose item is read, or Settings.
    fn title(&self, app: &App) -> String {
        let browsing = self.reader.browsing.read(app);
        let library = self.reader.library.read(app);
        let feed_title = |scope| match scope {
            Scope::All => None,
            Scope::Feed(id) => library.feed(id).map(|feed| feed.title.clone()),
        };
        let named = match browsing.page() {
            Page::Settings => return "Settings".to_owned(),
            Page::Reading(item) => feed_title(Scope::Feed(item.feed)),
            Page::Items => feed_title(browsing.scope()),
        };
        named.unwrap_or_else(|| "All Items".to_owned())
    }

    fn commands(&self, app: &App) -> Vec<WidgetRef> {
        let reader = &self.reader;
        let browsing = reader.browsing.read(app);
        match browsing.page() {
            Page::Settings => Vec::new(),
            Page::Reading(_) => vec![
                icon_button(icons::mark_unread(), "Mark as Unread", reader, keep_unread),
                icon_button(
                    icons::open_outside(),
                    "Open in Browser",
                    reader,
                    open_original,
                ),
            ],
            Page::Items => vec![
                icon_button(
                    icons::mark_read(),
                    "Mark All as Read",
                    reader,
                    mark_listed_read,
                ),
                self.refresh(app, browsing.scope()),
            ],
        }
    }

    /// Refresh, or a ring turning in its place while the feeds listed are being fetched.
    fn refresh(&self, app: &App, scope: Scope) -> WidgetRef {
        let shown = if self.reader.refreshing.read(app).is_under_way(scope) {
            turning_ring()
        } else {
            icon_button(icons::refresh(), "Refresh", &self.reader, refresh_listed)
        };
        SizedBox::new()
            .width(REFRESH_ROOM.0)
            .height(REFRESH_ROOM.1)
            .child(Center::new().child(shown))
            .into_widget()
    }
}

/// Fetches the feeds listed, as the user asked.
fn refresh_listed(app: &mut App, reader: &ReaderApp) {
    let scope = reader.browsing.read(app).scope();
    refresh(app, reader, scope, Asked::ByUser);
}

fn turning_ring() -> WidgetRef {
    SizedBox::new()
        .width(RING_SIZE)
        .height(RING_SIZE)
        .child(ProgressRing::new().is_active(true))
        .into_widget()
}
