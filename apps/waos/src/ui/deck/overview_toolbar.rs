//! The overview's toolbar, in the bar's place: a plus, which goes home, the number of tabs, and
//! Done, which goes back to the current tab's page. Cards scroll under it, so it is opaque, as
//! the bar is: a program's card is the system's layer beneath WaOS's picture, which WaOS's drawing
//! cannot blur.
//!
//! Its surface stands beneath the bar from the moment the current page leaves the strip, so
//! nothing of the grid ever shows through where the bars are. The bar fades away over it in the
//! first quarter of the page's flight, and the toolbar's controls fade in over the next: the two
//! bars' controls never show through each other.

use waft::{
    App, BuildContext, EdgeInsetsGeometry, FontWeight, Handle, Icon, IgnorePointer, IntoWidget,
    Listener, MainAxisAlignment, Opacity, Padding, Positioned, Row, Text, TextStyle, WidgetRef,
};
use waft_cupertino::{CupertinoButton, CupertinoColors, CupertinoIcons};

use super::bar::{BAR_GONE_AT, bar_material};
use super::{DeckViewState, StripFollows, words};
use crate::logic::tab_page::BAR_HEIGHT;
use crate::ui::library::home::cupertino::resolved;

/// How far the current page has flown when the toolbar's controls are whole; they start to show
/// once the bar is gone.
const CONTROLS_WHOLE_AT: f64 = 0.5;

impl DeckViewState {
    /// The toolbar, while the current page is off the strip at all.
    pub(super) fn overview_toolbar(
        self: Handle<Self>,
        app: &mut App,
        context: BuildContext,
    ) -> Option<WidgetRef> {
        let openness = self.openness(app);
        if openness <= 0.0 {
            return None;
        }
        let controls_shown =
            ((openness - BAR_GONE_AT) / (CONTROLS_WHOLE_AT - BAR_GONE_AT)).clamp(0.0, 1.0);
        let layout = self.layout(app);
        let tab_count = self.cards_staying(app).len();
        let label = resolved(CupertinoColors::LABEL, app, context);
        let row = Row::new()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .children([
                CupertinoButton::new(
                    Icon::new(Some(CupertinoIcons::add()))
                        .size(28.0)
                        .into_widget(),
                    Some(Listener::new(move |app| self.go_home(app))),
                )
                .into_widget(),
                Text::new(words::tab_count(tab_count))
                    .style(
                        TextStyle::new()
                            .font_size(15.0)
                            .font_weight(FontWeight::W600)
                            .color(label),
                    )
                    .into_widget(),
                CupertinoButton::new(
                    Text::new(words::DONE)
                        .style(TextStyle::new().font_weight(FontWeight::W600))
                        .into_widget(),
                    Some(Listener::new(move |app| self.close_overview(app))),
                )
                .into_widget(),
            ]);
        let toolbar = bar_material(
            Padding::new(EdgeInsetsGeometry::from_ltrb(
                4.0,
                0.0,
                4.0,
                layout.bar_extent() - BAR_HEIGHT,
            ))
            .child(Opacity::new(controls_shown).child(row))
            .into_widget(),
            app,
            context,
        );
        let toolbar = Positioned {
            left: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            height: Some(layout.bar_extent()),
            ..Positioned::new(
                IgnorePointer::new()
                    .ignoring(!self.overview_takes_touches(app))
                    .child(toolbar),
            )
        };
        Some(toolbar.into_widget())
    }

    /// The plus, and the system's Back on a program's page: the deck heads to the home's page, made
    /// at the end where no home is open, the home's card growing into it from the overview, and
    /// the strip gliding there from a page.
    pub(super) fn go_home(self: Handle<Self>, app: &mut App) {
        self.change_tabs(app, StripFollows::Unseen, |tabs, cx| {
            tabs.go_home(cx);
        });
    }
}
