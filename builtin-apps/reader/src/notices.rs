//! What the window says above its content, in Fluent's info bars: that a refresh could not reach
//! the feeds listed, with why and Try Again, while the list shows what the library has; and what
//! a command has to say, until the user closes it. Nothing while it has nothing to say.

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, IntoWidget, Listener,
    MainAxisSize, Padding, SizedBox, StatelessWidget, WidgetRef,
};
use waft_winui::{Button, InfoBar, InfoBarSeverity};

use crate::browsing::Page;
use crate::content_insets::content_insets;
use crate::reader_app::ReaderApp;
use crate::refreshing::{Asked, refresh};

/// The space between two bars and below the last, in logical points.
const GAP: f64 = 8.0;

#[derive(Debug)]
pub(crate) struct Notices {
    pub(crate) reader: ReaderApp,
}

impl StatelessWidget for Notices {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let bars: Vec<WidgetRef> = [
            not_refreshed_bar(app, &self.reader),
            notice_bar(app, &self.reader),
        ]
        .into_iter()
        .flatten()
        .map(IntoWidget::into_widget)
        .collect();
        if bars.is_empty() {
            return SizedBox::shrink().into_widget();
        }
        let column = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(GAP)
            .children(bars);
        let insets = content_insets(app, context, &self.reader);
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            insets.left,
            0.0,
            insets.right,
            GAP,
        ))
        .child(column)
        .into_widget()
    }
}

/// That the feeds listed could not be refreshed, and why, above the items the library has.
fn not_refreshed_bar(app: &App, reader: &ReaderApp) -> Option<InfoBar> {
    let browsing = reader.browsing.read(app);
    if *browsing.page() != Page::Items {
        return None;
    }
    let scope = browsing.scope();
    let refreshing = reader.refreshing.read(app);
    let library = reader.library.read(app);
    let problems: Vec<(String, &str)> = refreshing
        .problems(scope)
        .filter_map(|(id, why)| Some((library.feed(id)?.title.clone(), why)))
        .collect();
    let (first, why) = problems.first()?;
    let title = match problems.len() {
        1 => format!("“{first}” could not be refreshed."),
        several => format!("{several} feeds could not be refreshed."),
    };
    let retrying = reader.clone();
    let try_again = Button::text(
        "Try Again",
        Listener::new(move |app: &mut App| refresh(app, &retrying, scope, Asked::ByUser)),
    );
    let bar = InfoBar::new(true, |_, _| {})
        .severity(InfoBarSeverity::Warning)
        .title(title)
        .message(format!("{why} Reader shows what it has."))
        .action_button(try_again)
        .is_closable(false);
    Some(bar)
}

/// What a command has to say, until the user closes it.
fn notice_bar(app: &App, reader: &ReaderApp) -> Option<InfoBar> {
    let notice = reader.browsing.read(app).notice()?.clone();
    let closing = reader.clone();
    let bar = InfoBar::new(true, move |app, open| {
        if !open {
            closing.browsing.update(app, |browsing, cx| {
                browsing.clear_notice();
                cx.notify();
            });
        }
    })
    .severity(notice.severity)
    .message(notice.message);
    Some(bar)
}
