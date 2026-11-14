//! An item being read, under the header that names its feed: its title, when it was published, its
//! picture where its text shows none, then its text, laid out block by block (`article_view`) in a column a
//! line of text reads well in, and at its foot the way to the original in the system's browser.
//! The page builds only the blocks in view, however long the item.
//!
//! The item's HTML is read into blocks once, as the page opens on it, not at each build.

use std::rc::Rc;

use waft::{
    Align, AlignmentGeometry, App, BoxConstraints, BuildContext, Column, ConstrainedBox,
    CrossAxisAlignment, EdgeInsetsGeometry, Handle, IntoWidget, ListView, Listener, MainAxisSize,
    Padding, SizedBox, State, StateData, StatefulWidget, Text, WidgetRef,
};
use waft_winui::{HyperlinkButton, TextBlockStyle, ThemeResources};

use crate::article::{self, Block};
use crate::article_html::blocks_of;
use crate::article_view::{block_view, picture_view};
use crate::content_insets::content_insets;
use crate::dates;
use crate::library::{ItemRef, Listed};
use crate::opening_links::open_original;
use crate::reader_app::ReaderApp;
use crate::refreshing::seconds_now;

/// The widest the text's column is, in logical points.
const WIDEST: f64 = 700.0;

#[derive(Debug)]
pub(crate) struct ReadingPage {
    pub(crate) reader: ReaderApp,
    pub(crate) item: ItemRef,
}

impl StatefulWidget for ReadingPage {
    type State = ReadingPageState;

    fn create_state(&self) -> ReadingPageState {
        ReadingPageState {
            state: StateData::new(),
            blocks: Rc::new(Vec::new()),
        }
    }
}

pub(crate) struct ReadingPageState {
    state: StateData<ReadingPage>,
    /// The item's text, read into blocks.
    blocks: Rc<Vec<Block>>,
}

impl ReadingPageState {
    /// Reads the text of the item the widget names into blocks.
    fn read_blocks(self: Handle<Self>, app: &mut App) {
        let widget = self.widget(app);
        let library = widget.reader.library.read(app);
        let blocks = library
            .item(&widget.item)
            .map(blocks_read)
            .unwrap_or_default();
        app.get_mut(self).blocks = Rc::new(blocks);
    }
}

impl State for ReadingPageState {
    type Widget = ReadingPage;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        self.read_blocks(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old_widget: &ReadingPage) {
        if old_widget.item != self.widget(app).item {
            self.read_blocks(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let widget = self.widget(app);
        let reader = widget.reader.clone();
        let Some(listed) = reader.library.read(app).item(&widget.item) else {
            return SizedBox::shrink().into_widget();
        };
        let blocks = Rc::clone(&app.get(self).blocks);
        let head = Rc::new(Head::of(listed, &blocks));
        let insets = content_insets(app, context, &reader);
        let last = blocks.len() + 1;
        let list = ListView::builder(move |app, context, index| {
            let part = match usize::try_from(index).ok()? {
                0 => head.view(app, context, &reader),
                index if index == last => foot(&reader, head.has_original),
                index => block_view(app, context, &reader, blocks.get(index - 1)?),
            };
            Some(in_column(part))
        })
        .item_count(i32::try_from(last + 1).unwrap_or(i32::MAX))
        .build()
        .padding(EdgeInsetsGeometry::from_ltrb(
            insets.left,
            8.0,
            insets.right,
            48.0,
        ));
        list.into_widget()
    }
}

/// The text of `listed` as blocks, without the headline that only says its title again.
fn blocks_read(listed: Listed<'_>) -> Vec<Block> {
    let base = listed.item.link.as_deref().unwrap_or(&listed.feed.address);
    let base = url::Url::parse(base).ok();
    let mut blocks = blocks_of(&listed.item.html, base.as_ref());
    article::drop_repeated_title(&mut blocks, &listed.item.title);
    blocks
}

/// What the page says above the item's text.
struct Head {
    title: String,
    /// When the item was published.
    line: String,
    /// The item's picture, where its text shows none.
    picture: Option<String>,
    has_original: bool,
}

impl Head {
    fn of(listed: Listed<'_>, blocks: &[Block]) -> Head {
        let published = dates::ago(listed.item.listed_at(), seconds_now(), dates::users_offset);
        let shown_in_text = article::first_picture(blocks).is_some();
        Head {
            title: listed.item.title.clone(),
            line: published,
            picture: listed.item.picture.clone().filter(|_| !shown_in_text),
            has_original: listed.item.link.is_some(),
        }
    }

    fn view(&self, app: &mut App, context: BuildContext, reader: &ReaderApp) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let title = TextBlockStyle::Title.text_style(&resources, resources.text_fill_color_primary);
        let line = TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary);
        let mut parts = vec![
            Text::new(self.title.clone())
                .style(title.height(1.25))
                .into_widget(),
            Text::new(self.line.clone()).style(line).into_widget(),
        ];
        if let Some(picture) = &self.picture {
            let picture = picture_view(reader, picture, "");
            let below = Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 12.0, 0.0, 0.0));
            parts.push(below.child(picture).into_widget());
        }
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(8.0)
            .children(parts)
            .into_widget()
    }
}

/// The way to the original, beneath the text, where the item has one.
fn foot(reader: &ReaderApp, has_original: bool) -> WidgetRef {
    if !has_original {
        return SizedBox::shrink().into_widget();
    }
    let reader = reader.clone();
    let open = HyperlinkButton::text(
        "Open in Browser",
        Listener::new(move |app: &mut App| open_original(app, &reader)),
    );
    Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 28.0, 0.0, 0.0))
        .child(
            Align::new()
                .alignment(AlignmentGeometry::CENTER_LEFT)
                .child(open),
        )
        .into_widget()
}

/// `part`, at the left of its room, in a column no wider than [`WIDEST`].
fn in_column(part: WidgetRef) -> WidgetRef {
    Align::new()
        .alignment(AlignmentGeometry::TOP_LEFT)
        .child(ConstrainedBox::new(BoxConstraints::new().max_width(WIDEST)).child(part))
        .into_widget()
}
