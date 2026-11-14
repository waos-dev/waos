//! Text said aside, in WinUI's caption style and its secondary colour: an item's line beneath its
//! title, and the line above the text shown, when the note last changed or where the file is.

use waft_foundation::App;
use waft_widgets::{BuildContext, Text, TextOverflow};
use waft_winui::{TextBlockStyle, ThemeResources};

/// `text`, said aside on one line, cut short with an ellipsis where it is too long.
pub(crate) fn secondary_text(
    app: &mut App,
    context: BuildContext,
    text: impl Into<String>,
) -> Text {
    let resources = ThemeResources::of(app, context);
    let style = TextBlockStyle::Caption.text_style(&resources, resources.text_fill_color_secondary);
    Text::new(text)
        .style(style)
        .max_lines(1)
        .overflow(TextOverflow::Ellipsis)
}
