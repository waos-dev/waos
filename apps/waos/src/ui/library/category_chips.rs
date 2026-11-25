//! A row of category chips in Fluent's controls, which the home and the app store both show
//! while no category is chosen: one chip a category, a press on it choosing it. Each is a button
//! drawn as macOS's Apps draws its chips, quiet beside what they narrow.

use std::rc::Rc;

use waft::{
    App, Axis, BorderSide, BuildContext, DecoratedBox, EdgeInsetsGeometry, IntoWidget, Listener,
    Padding, Row, ShapeDecoration, SingleChildScrollView, StadiumBorder, Text, TextFieldTapRegion,
    WidgetRef,
};
use waft_winui::{Button, CommonState, ControlStates, TextBlockStyle, ThemeResources};
use waos_package::Category;

use super::words;

/// The room between two category chips, and under their row.
const CHIP_SPACING: f64 = 8.0;
const CHIPS_GAP: f64 = 12.0;
/// The room inside a chip around its name: across, and up and down.
const CHIP_PADDING: [f64; 2] = [12.0, 4.0];
/// How much of the text's colour fills a chip: at rest, and pressed.
const CHIP_FILL: f64 = 0.05;
const CHIP_FILL_PRESSED: f64 = 0.12;

/// A chip for each of `categories`, in a row `margin` in from each side that scrolls sideways
/// where the chips outgrow it, as macOS's Apps scrolls its own. A press tells `chosen` the
/// category to show alone.
pub(crate) fn category_chips(
    categories: &[Category],
    margin: f64,
    chosen: impl Fn(&mut App, Category) + 'static,
) -> WidgetRef {
    let chosen = Rc::new(chosen);
    let chips: Vec<WidgetRef> = categories
        .iter()
        .map(|&category| {
            let chosen = Rc::clone(&chosen);
            let choose = Listener::new(move |app| chosen(app, category));
            Button::text(words::category(category), choose)
                .template(move |app, context, states, _content| {
                    category_chip(app, context, category, states)
                })
                .into_widget()
        })
        .collect();
    let row = SingleChildScrollView::new()
        .scroll_direction(Axis::Horizontal)
        .padding(EdgeInsetsGeometry::from_ltrb(
            margin, 0.0, margin, CHIPS_GAP,
        ))
        .child(Row::new().spacing(CHIP_SPACING).children(chips));
    // The chips are part of the search field their category goes to as a token, as Flutter has
    // a field's own controls be: a press on one is no press outside the field, which would take
    // the keyboard from it, so the user types on, or takes the token back.
    TextFieldTapRegion::new(row).into_widget()
}

/// A category's chip as macOS's Apps draws its own, quiet beside the icons: a capsule with no
/// border, a faint fill and the name in the secondary text colour, the fill a shade stronger
/// while pressed. The fill is the text's own colour, thinned, so it reads over the card whether
/// the card is solid or a tint over the system's blur.
fn category_chip(
    app: &mut App,
    context: BuildContext,
    category: Category,
    states: ControlStates,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let fill = if states.common == CommonState::Pressed {
        CHIP_FILL_PRESSED
    } else {
        CHIP_FILL
    };
    let capsule = ShapeDecoration::new(StadiumBorder::new(BorderSide::NONE)).color(
        resources
            .text_fill_color_primary
            .with_values(Some(fill), None, None, None, None),
    );
    let name = Text::new(words::category(category))
        .style(TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary));
    DecoratedBox::new(capsule)
        .child(
            Padding::new(EdgeInsetsGeometry::symmetric(
                CHIP_PADDING[1],
                CHIP_PADDING[0],
            ))
            .child(name),
        )
        .into_widget()
}
