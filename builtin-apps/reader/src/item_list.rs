//! The items listed, newest first, each a row (`item_row`): every feed's, or one feed's. The list
//! builds only the rows in view, however many items the library keeps, and keeps to a column no
//! wider than a line of text reads well in. Where there is nothing to list it says why: no feed
//! is followed, with Add Feed; the feeds are being fetched for the first time; or the feed lists
//! nothing.

use std::rc::Rc;

use waft::{
    Align, AlignmentGeometry, App, BoxConstraints, BuildContext, Center, Column, ConstrainedBox,
    EdgeInsetsGeometry, IntoWidget, ListView, Listener, MainAxisSize, Padding, StatelessWidget,
    Text, ValueKey, WidgetRef,
};
use waft_winui::{Button, ProgressRing, TextBlockStyle, ThemeResources};

use crate::adding::ask_for_a_feed;
use crate::content_insets::content_insets;
use crate::dates;
use crate::item_row::{DOT_ROOM, ItemRow, ROW_PADDING, RowData};
use crate::library::{Listed, Scope};
use crate::reader_app::ReaderApp;
use crate::refreshing::seconds_now;

/// The widest the list's column is, in logical points.
const WIDEST: f64 = 820.0;

#[derive(Debug)]
pub(crate) struct ItemList {
    pub(crate) reader: ReaderApp,
}

impl StatelessWidget for ItemList {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let scope = self.reader.browsing.read(app).scope();
        let rows = self.rows(app, scope);
        if rows.is_empty() {
            return self.nothing_listed(app, context, scope);
        }
        let insets = content_insets(app, context, &self.reader);
        // A row's words begin in line with the header's title, its dot in the margin before them.
        let left = (insets.left - DOT_ROOM).max(0.0);
        let right = (insets.right - ROW_PADDING).max(0.0);
        let count = i32::try_from(rows.len()).unwrap_or(i32::MAX);
        let (reader, rows) = (self.reader.clone(), Rc::new(rows));
        let list = ListView::builder(move |_app, _context, index| {
            let data = rows.get(usize::try_from(index).ok()?)?.clone();
            let key = ValueKey::new(data.item.clone());
            let row = ItemRow {
                reader: reader.clone(),
                data,
            };
            Some(in_column(row).key(key.into()).into_widget())
        })
        .item_count(count)
        .build()
        .padding(EdgeInsetsGeometry::from_ltrb(left, 0.0, right, 12.0));
        list.into_widget()
    }
}

impl ItemList {
    /// What each row shows of the items `scope` lists, newest first.
    fn rows(&self, app: &App, scope: Scope) -> Vec<RowData> {
        let now = seconds_now();
        let library = self.reader.library.read(app);
        let listed = library.listed(scope);
        listed
            .into_iter()
            .map(|listed| row_of(listed, scope, now))
            .collect()
    }

    /// What the list says where it lists nothing.
    fn nothing_listed(&self, app: &mut App, context: BuildContext, scope: Scope) -> WidgetRef {
        let no_feeds = self.reader.library.read(app).feeds().next().is_none();
        let fetching = self.reader.refreshing.read(app).is_under_way(scope);
        if no_feeds {
            let reader = self.reader.clone();
            let add = Button::text(
                "Add Feed",
                Listener::new(move |app: &mut App| ask_for_a_feed(app, &reader)),
            );
            return said(
                app,
                context,
                "You follow no feeds yet.",
                Some(add.into_widget()),
            );
        }
        if fetching {
            let ring = ProgressRing::new().is_active(true);
            return Center::new().child(ring).into_widget();
        }
        said(app, context, "Nothing here yet.", None)
    }
}

/// What a row shows of `listed`: beneath its text, the feed it is of, unless the list is that
/// feed's alone, and when it was published.
fn row_of(listed: Listed<'_>, scope: Scope, now: u64) -> RowData {
    let published = dates::ago(listed.item.listed_at(), now, dates::users_offset);
    let line = match scope {
        Scope::All => format!("{} · {published}", listed.feed.title),
        Scope::Feed(_) => published,
    };
    RowData {
        item: listed.item_ref(),
        title: listed.item.title.clone(),
        preview: listed.item.preview.clone(),
        line,
        picture: listed.item.picture.clone(),
        read: listed.item.read,
    }
}

/// `row`, at the left of its room, in a column no wider than [`WIDEST`].
fn in_column(row: ItemRow) -> Align {
    Align::new()
        .alignment(AlignmentGeometry::TOP_LEFT)
        .child(ConstrainedBox::new(BoxConstraints::new().max_width(WIDEST)).child(row))
}

/// `line`, said in the middle of the list's room, with `action` beneath it where given.
fn said(app: &mut App, context: BuildContext, line: &str, action: Option<WidgetRef>) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let style = TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary);
    let mut parts = vec![Text::new(line).style(style).into_widget()];
    parts.extend(action);
    let column = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(12.0)
        .children(parts);
    Center::new()
        .child(Padding::new(EdgeInsetsGeometry::all(24.0)).child(column))
        .into_widget()
}
