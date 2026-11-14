//! The settings: whether a notification tells of new items, and the feeds followed, each with its
//! address and the way to unfollow it.

use waft::{
    Align, AlignmentGeometry, App, BoxConstraints, BuildContext, Column, ConstrainedBox,
    CrossAxisAlignment, EdgeInsetsGeometry, Expanded, IntoWidget, Listener, MainAxisSize, Row,
    SingleChildScrollView, SizedBox, StatelessWidget, Text, TextOverflow, WidgetRef,
};
use waft_winui::{Button, TextBlockStyle, ThemeResources, ToggleSwitch};

use crate::content_insets::content_insets;
use crate::feed::Feed;
use crate::new_item_notices::set_notifying;
use crate::reader_app::ReaderApp;
use crate::unfollowing::ask_to_unfollow;

/// The widest the settings' column is, in logical points.
const WIDEST: f64 = 640.0;

const NOTIFYING: &str = "Tell me about new items";
const NOTIFYING_DETAIL: &str = "Reader refreshes every half hour while it runs, in sight or not. With this on, a notification says when it finds something new.";

#[derive(Debug)]
pub(crate) struct SettingsPage {
    pub(crate) reader: ReaderApp,
}

impl StatelessWidget for SettingsPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let mut parts = vec![
            section_title(&resources, "Notifications"),
            self.notifying(app, &resources),
            SizedBox::new().height(20.0).into_widget(),
            section_title(&resources, "Feeds"),
        ];
        let feeds: Vec<Feed> = self.reader.library.read(app).feeds().cloned().collect();
        if feeds.is_empty() {
            parts.push(aside(&resources, "You follow no feeds."));
        }
        parts.extend(feeds.iter().map(|feed| self.feed_row(&resources, feed)));
        let column = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(10.0)
            .children(parts);
        let page = ConstrainedBox::new(BoxConstraints::new().max_width(WIDEST)).child(column);
        let insets = content_insets(app, context, &self.reader);
        SingleChildScrollView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(
                insets.left,
                8.0,
                insets.right,
                32.0,
            ))
            .child(
                Align::new()
                    .alignment(AlignmentGeometry::TOP_LEFT)
                    .child(page),
            )
            .into_widget()
    }
}

impl SettingsPage {
    /// The switch that turns the notification of new items on and off, with what it does.
    fn notifying(&self, app: &App, resources: &ThemeResources) -> WidgetRef {
        let on = self.reader.settings.read(app).notify_of_new_items;
        let reader = self.reader.clone();
        let switch = ToggleSwitch::new(on, move |app, on| set_notifying(app, &reader, on))
            .header(Text::new(NOTIFYING));
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(6.0)
            .children([switch.into_widget(), aside(resources, NOTIFYING_DETAIL)])
            .into_widget()
    }

    /// A feed by its title and, beneath it, its address, with Unfollow.
    fn feed_row(&self, resources: &ThemeResources, feed: &Feed) -> WidgetRef {
        let title = TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary);
        let named = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .spacing(2.0)
            .children([
                Text::new(feed.title.clone())
                    .style(title)
                    .max_lines(1)
                    .overflow(TextOverflow::Ellipsis)
                    .into_widget(),
                aside(resources, &feed.address),
            ]);
        let (reader, id) = (self.reader.clone(), feed.id);
        let unfollow = Button::text(
            "Unfollow",
            Listener::new(move |app: &mut App| ask_to_unfollow(app, &reader, id)),
        );
        Row::new()
            .children([
                Expanded::new(named).into_widget(),
                SizedBox::new().width(12.0).into_widget(),
                unfollow.into_widget(),
            ])
            .into_widget()
    }
}

fn section_title(resources: &ThemeResources, title: &str) -> WidgetRef {
    let style = TextBlockStyle::BodyStrong.text_style(resources, resources.text_fill_color_primary);
    Text::new(title).style(style).into_widget()
}

/// `text`, said aside in WinUI's caption style and its secondary colour, on up to two lines.
fn aside(resources: &ThemeResources, text: &str) -> WidgetRef {
    let style = TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary);
    Text::new(text)
        .style(style)
        .max_lines(2)
        .overflow(TextOverflow::Ellipsis)
        .into_widget()
}
