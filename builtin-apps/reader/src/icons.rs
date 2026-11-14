//! The Fluent icons the app shows: those `FluentSymbol` does not name by their code points in the
//! icon font `waft-winui` bundles, the 20-point regular ones its symbols are.

use waft_winui::{FluentSymbol, FontIcon};

/// `ic_fluent_news_20_regular`: every feed's items.
const NEWS: char = '\u{F565}';

/// `ic_fluent_rss_20_regular`: a feed.
const RSS: char = '\u{EA3D}';

/// `ic_fluent_arrow_clockwise_20_regular`: Refresh.
const ARROW_CLOCKWISE: char = '\u{F13D}';

/// `ic_fluent_open_20_regular`: Open in Browser.
const OPEN: char = '\u{F582}';

/// `ic_fluent_mail_read_20_regular`: Mark All as Read.
const MAIL_READ: char = '\u{F521}';

/// `ic_fluent_mail_unread_20_regular`: Mark as Unread.
const MAIL_UNREAD: char = '\u{F526}';

/// `ic_fluent_image_off_20_regular`: a picture that could not be shown.
const IMAGE_OFF: char = '\u{E72C}';

/// The size icons are shown at in the list of feeds and above the content, in logical points.
const ICON_SIZE: f64 = 16.0;

pub(crate) fn all_items() -> FontIcon {
    icon(NEWS)
}

pub(crate) fn feed() -> FontIcon {
    icon(RSS)
}

pub(crate) fn add() -> FontIcon {
    FontIcon::symbol(FluentSymbol::Add).font_size(ICON_SIZE)
}

pub(crate) fn refresh() -> FontIcon {
    icon(ARROW_CLOCKWISE)
}

pub(crate) fn open_outside() -> FontIcon {
    icon(OPEN)
}

pub(crate) fn mark_read() -> FontIcon {
    icon(MAIL_READ)
}

pub(crate) fn mark_unread() -> FontIcon {
    icon(MAIL_UNREAD)
}

/// The mark of a picture that could not be shown, at `size` logical points.
pub(crate) fn no_picture(size: f64) -> FontIcon {
    FontIcon::new(IMAGE_OFF).font_size(size)
}

fn icon(glyph: char) -> FontIcon {
    FontIcon::new(glyph).font_size(ICON_SIZE)
}
