//! What the overview draws around each card: its title beneath it and its close button on its
//! corner.
//!
//! They are painted with their card, right after its frame, so a page that flies over a card
//! covers them as it covers the card. They take no touches: the close button's are the touch
//! layer's, over every page.

use waft::{
    Align, AlignmentGeometry, App, BoxDecoration, BoxShape, BuildContext, Color, DecoratedBox,
    FontWeight, Icon, IgnorePointer, IntoWidget, KeyedSubtree, MainAxisAlignment, Opacity,
    Positioned, Rect, Row, SizedBox, Text, TextOverflow, TextStyle, ValueKey, WidgetRef,
};
use waft_cupertino::{CupertinoColors, CupertinoIcons};

use super::frames::PlacedTab;
use super::words;
use crate::logic::tab_list::TabId;
use crate::ui::library::home::cupertino::resolved;

const CLOSE_BUTTON_SIDE: f64 = 26.0;
/// How far in from the card's corner its close button is.
const CLOSE_BUTTON_INSET: f64 = 7.0;
const TITLE_HEIGHT: f64 = 20.0;
/// Between a card and its title.
const TITLE_GAP: f64 = 7.0;
/// The side of the program's icon beside a card's title.
const TITLE_ICON_SIDE: f64 = 16.0;

/// The parts of a card's chrome, for keys unique among the deck's children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ChromePart {
    Title,
    CloseButton,
}

/// The title and the close button of `card`, while they show at all; nothing for the page past
/// the last tab, which has no card.
pub(super) fn card_chrome(
    card: &PlacedTab,
    app: &mut App,
    context: BuildContext,
) -> Vec<WidgetRef> {
    let Some(tab) = card.tab.tab else {
        return Vec::new();
    };
    if card.placement.chrome_opacity <= 0.0 {
        return Vec::new();
    }
    vec![
        drawn(
            tab,
            ChromePart::Title,
            title_area(card),
            card,
            title(card, app, context),
        ),
        drawn(
            tab,
            ChromePart::CloseButton,
            close_button_area(card),
            card,
            cross(),
        ),
    ]
}

/// Where the close button of `card` is: on the card's top right corner.
pub(super) fn close_button_area(card: &PlacedTab) -> Rect {
    let clip = card.placement.clip;
    Rect::from_ltwh(
        clip.right - CLOSE_BUTTON_SIDE - CLOSE_BUTTON_INSET,
        clip.top + CLOSE_BUTTON_INSET,
        CLOSE_BUTTON_SIDE,
        CLOSE_BUTTON_SIDE,
    )
}

fn title_area(card: &PlacedTab) -> Rect {
    let clip = card.placement.clip;
    Rect::from_ltwh(
        clip.left,
        clip.bottom + TITLE_GAP,
        clip.width().max(0.0),
        TITLE_HEIGHT,
    )
}

/// `part` of the chrome of `tab`, drawn in `area` as opaque as the card's chrome is.
fn drawn(
    tab: TabId,
    part: ChromePart,
    area: Rect,
    card: &PlacedTab,
    part_drawn: WidgetRef,
) -> WidgetRef {
    let faded = Opacity::new(card.placement.chrome_opacity).child(part_drawn);
    KeyedSubtree::new(Positioned::from_rect(
        area,
        IgnorePointer::new().child(faded),
    ))
    .key(ValueKey::new((tab, part)).into())
    .into_widget()
}

/// The title of `card`: its program's icon and name, or what names the home.
fn title(card: &PlacedTab, app: &mut App, context: BuildContext) -> WidgetRef {
    let label = resolved(CupertinoColors::LABEL, app, context);
    let mut row = Vec::new();
    let name = match &card.tab.program {
        Some(program) => {
            row.push(program.icon(TITLE_ICON_SIDE));
            row.push(SizedBox::new().width(6.0).into_widget());
            program.name.clone()
        }
        None => words::HOME.to_owned(),
    };
    row.push(
        waft::Flexible::new(
            Text::new(name)
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis)
                .style(
                    TextStyle::new()
                        .font_size(13.0)
                        .font_weight(FontWeight::W500)
                        .color(label),
                ),
        )
        .into_widget(),
    );
    Row::new()
        .main_axis_alignment(MainAxisAlignment::Center)
        .children(row)
        .into_widget()
}

/// The close button's cross on its dark disc.
fn cross() -> WidgetRef {
    DecoratedBox::new(BoxDecoration {
        color: Some(Color::from_argb(110, 0, 0, 0).into()),
        shape: BoxShape::Circle,
        ..BoxDecoration::default()
    })
    .child(
        Align::new().alignment(AlignmentGeometry::CENTER).child(
            Icon::new(Some(CupertinoIcons::xmark()))
                .size(13.0)
                .color(Color::new(0xFFFF_FFFF)),
        ),
    )
    .into_widget()
}
