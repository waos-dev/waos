//! A row of category chips in Fluent's controls, which the home and the app store both show: one
//! chip a category, the chosen one on, a press on it choosing none. Each is a toggle button drawn
//! as macOS's Apps draws its chips, quiet beside what they narrow.

use std::rc::Rc;

use waft::{
    App, Axis, BorderSide, BuildContext, DecoratedBox, EdgeInsetsGeometry, IntoWidget, Padding,
    Row, ShapeDecoration, SingleChildScrollView, StadiumBorder, Text, WidgetRef,
};
use waft_winui::{CommonState, ControlStates, TextBlockStyle, ThemeResources, ToggleButton};
use waos_package::Category;

use super::words;

/// The room between two category chips, and under their row.
const CHIP_SPACING: f64 = 8.0;
const CHIPS_GAP: f64 = 12.0;
/// The room inside a chip around its name: across, and up and down.
const CHIP_PADDING: [f64; 2] = [12.0, 4.0];
/// How much of the text's colour fills a chip: at rest, on (or pressed), and on while pressed.
const CHIP_FILL: f64 = 0.05;
const CHIP_FILL_ON: f64 = 0.12;
const CHIP_FILL_ON_PRESSED: f64 = 0.16;

/// A chip for each of `categories`, `shown` the one that is on, in a row `margin` in from each
/// side that scrolls sideways where the chips outgrow it, as macOS's Apps scrolls its own. A
/// press tells `chosen` the category to show alone: the chip's, or none for the one that is on.
pub(crate) fn category_chips(
    categories: &[Category],
    shown: Option<Category>,
    margin: f64,
    chosen: impl Fn(&mut App, Option<Category>) + 'static,
) -> WidgetRef {
    let chosen = Rc::new(chosen);
    let chips: Vec<WidgetRef> = categories
        .iter()
        .map(|&category| {
            let on = shown == Some(category);
            let chosen = Rc::clone(&chosen);
            ToggleButton::new(Some(on), move |app, _| {
                chosen(app, (!on).then_some(category))
            })
            .template(move |app, context, on, _content, states| {
                category_chip(app, context, category, on == Some(true), states)
            })
            .into_widget()
        })
        .collect();
    SingleChildScrollView::new()
        .scroll_direction(Axis::Horizontal)
        .padding(EdgeInsetsGeometry::from_ltrb(
            margin, 0.0, margin, CHIPS_GAP,
        ))
        .child(Row::new().spacing(CHIP_SPACING).children(chips))
        .into_widget()
}

/// A category's chip as macOS's Apps draws its own, quiet beside the icons: a capsule with no
/// border, a faint fill and the name in the secondary text colour; `on`, a stronger fill and the
/// primary colour, and a shade stronger while pressed. The fill is the text's own colour, thinned,
/// so it reads over the card whether the card is solid or a tint over the system's blur.
fn category_chip(
    app: &mut App,
    context: BuildContext,
    category: Category,
    on: bool,
    states: ControlStates,
) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let pressed = states.common == CommonState::Pressed;
    let fill = match (on, pressed) {
        (false, false) => CHIP_FILL,
        (false, true) | (true, false) => CHIP_FILL_ON,
        (true, true) => CHIP_FILL_ON_PRESSED,
    };
    let name_color = if on {
        resources.text_fill_color_primary
    } else {
        resources.text_fill_color_secondary
    };
    let capsule = ShapeDecoration::new(StadiumBorder::new(BorderSide::NONE)).color(
        resources
            .text_fill_color_primary
            .with_values(Some(fill), None, None, None, None),
    );
    let name = Text::new(words::category(category))
        .style(TextBlockStyle::Body.text_style(&resources, name_color));
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
