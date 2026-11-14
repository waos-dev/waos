//! The bar along the bottom, as Safari's on a phone: a pill naming the page, and the tabs button.
//! Each page on the strip has its own pill, which slides with its page, so a swipe on the bar
//! drags the name along with the page. A program's pill is its icon and name; the home's is a
//! search field (`search_pill`), and while that field has the keyboard the bar sits above it. The bar is opaque: it lies over nothing of a program's, and
//! WaOS's drawing could not blur a program's picture, the system's layer beneath it, if it did.
//!
//! A sideways drag on the bar moves the strip (`strip`); an upward one takes the page into flight
//! to its card (`overview_transition`). The two compete as any two gestures of one detector do, so
//! the axis the finger takes first wins, and the tabs button's tap loses to either once the finger
//! moves. The bar fades away over the overview's toolbar, whose surface stands beneath it, in the
//! first quarter of the page's flight, and takes touches while any of it shows.
//!
//! The invariant: the bar's detector recognizes the same gestures at every build (`axis_drag`).

use waft::{
    AnyColor, App, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration,
    BuildContext, Clip, ClipRect, Color, DecoratedBox, EdgeInsetsGeometry, FontWeight,
    GestureDetector, Handle, Icon, IgnorePointer, IntoWidget, KeyedSubtree, Listener,
    MainAxisAlignment, MediaQuery, Opacity, Padding, Positioned, Row, Size, SizedBox, Stack, Text,
    TextOverflow, TextStyle, ValueKey, WidgetRef,
};
use waft_cupertino::{CupertinoButton, CupertinoColors, CupertinoDynamicColor, CupertinoIcons};
use waft_rendering::HitTestBehavior;

use super::axis_drag::AxisDragCallbacks;
use super::frames::PlacedTab;
use super::grip::Owner;
use super::pages::NamedProgram;
use super::{DeckViewState, ShownTab};
use crate::logic::tab_page::BAR_HEIGHT;
use crate::ui::library::home::cupertino::resolved;

const SIDE_MARGIN: f64 = 12.0;
const PILL_HEIGHT: f64 = 42.0;
pub(super) const PILL_RADIUS: f64 = 12.0;
const TABS_BUTTON_SIDE: f64 = 44.0;
/// Between the pills and the tabs button.
const BUTTON_GAP: f64 = 8.0;
/// The side of the program's icon in its pill.
const PILL_ICON_SIDE: f64 = 20.0;
/// How far the current page has flown when the bar has faded away over the toolbar beneath it,
/// and takes no more touches.
pub(super) const BAR_GONE_AT: f64 = 0.25;

/// The bar's own surface, in both brightnesses: the tint of iOS's bars, opaque.
const BAR_COLOR_DYNAMIC: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color::new(0xFFF9F9F9), Color::new(0xFF1D1D1D));
const BAR_COLOR: AnyColor = BAR_COLOR_DYNAMIC.to_any();
/// The pill's surface over the bar's.
const PILL_COLOR_DYNAMIC: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color::new(0xFFFFFFFF), Color::new(0xFF3A3A3C));
pub(super) const PILL_COLOR: AnyColor = PILL_COLOR_DYNAMIC.to_any();

impl DeckViewState {
    /// The bar, with a pill for each of `placed` that is on the strip.
    pub(super) fn bottom_bar(
        self: Handle<Self>,
        app: &mut App,
        context: BuildContext,
        placed: &[PlacedTab],
    ) -> WidgetRef {
        let layout = self.layout(app);
        let row = Row::new()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .children([
                self.pill_track(app, context, placed),
                tabs_button(Listener::new(move |app| self.open_overview(app))),
            ]);
        let material = bar_material(
            Padding::new(EdgeInsetsGeometry::from_ltrb(
                SIDE_MARGIN,
                0.0,
                SIDE_MARGIN,
                layout.bar_extent() - BAR_HEIGHT,
            ))
            .child(row)
            .into_widget(),
            app,
            context,
        );
        let bar = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .horizontal_axis_drag(self.strip_drag(Owner::Bar))
            .vertical_axis_drag(self.vertical_drag(Owner::Bar))
            .child(material);
        let openness = self.openness(app);
        let shown = (1.0 - openness / BAR_GONE_AT).clamp(0.0, 1.0);
        Positioned {
            left: Some(0.0),
            right: Some(0.0),
            bottom: Some(self.lift_over_the_keyboard(app, context)),
            height: Some(layout.bar_extent()),
            ..Positioned::new(
                IgnorePointer::new()
                    .ignoring(shown == 0.0)
                    .child(Opacity::new(shown).child(bar)),
            )
        }
        .into_widget()
    }

    /// How far the bar is lifted off the view's bottom: clear of the keyboard while its search
    /// field has it, so the field stays in sight, what the bar sits clear of beneath its own being
    /// under the keyboard then; not at all otherwise.
    fn lift_over_the_keyboard(self: Handle<Self>, app: &mut App, context: BuildContext) -> f64 {
        if !self.search_has_the_keyboard(app) {
            return 0.0;
        }
        let keyboard = MediaQuery::view_insets_of(app, context).bottom;
        (keyboard - self.layout(app).beneath_the_bar()).max(0.0)
    }

    /// The pill of `tab`: its program's, or the home's search field.
    fn pill(self: Handle<Self>, tab: &ShownTab, app: &mut App, context: BuildContext) -> WidgetRef {
        match &tab.program {
            Some(program) => program_pill(program, app, context),
            None => self.search_pill(app, context),
        }
    }

    /// One pill for each page on the strip, each as far from its place as its page is from the
    /// page's box, clipped to the pills' track while any is out of its place: a clip that cuts
    /// nothing still costs the renderer a pass over the whole picture, so the track's clip
    /// stays in the tree and clips only while the strip is off a page.
    fn pill_track(
        self: Handle<Self>,
        app: &mut App,
        context: BuildContext,
        placed: &[PlacedTab],
    ) -> WidgetRef {
        let layout = self.layout(app);
        let strip = self.strip(app).value(app);
        let track = Size::new(
            (layout.page_box().width() - 2.0 * SIDE_MARGIN - TABS_BUTTON_SIDE - BUTTON_GAP)
                .max(0.0),
            PILL_HEIGHT,
        );
        let on_the_strip: Vec<(f64, &PlacedTab)> = placed
            .iter()
            .enumerate()
            .map(|(index, tab)| (layout.pill_offset(index, strip), tab))
            .filter(|(offset, _)| offset.abs() < layout.page_spacing())
            .collect();
        let clip_behavior = if on_the_strip.iter().any(|(offset, _)| *offset != 0.0) {
            Clip::HardEdge
        } else {
            Clip::None
        };
        let pills: Vec<WidgetRef> = on_the_strip
            .into_iter()
            .map(|(offset, tab)| {
                let pill = Positioned {
                    left: Some(offset),
                    top: Some(0.0),
                    width: Some(track.width()),
                    height: Some(track.height()),
                    ..Positioned::new(self.pill(tab.tab, app, context))
                };
                KeyedSubtree::new(pill)
                    .key(ValueKey::new(tab.tab.key).into())
                    .into_widget()
            })
            .collect();
        let track_of_pills = ClipRect::new()
            .clip_behavior(clip_behavior)
            .child(Stack::new().clip_behavior(Clip::None).children(pills));
        SizedBox::new()
            .width(track.width())
            .height(track.height())
            .child(track_of_pills)
            .into_widget()
    }
}

/// The pill of `program`: its icon and its name.
fn program_pill(program: &NamedProgram, app: &mut App, context: BuildContext) -> WidgetRef {
    let label = resolved(CupertinoColors::LABEL, app, context);
    let name = Text::new(program.name.clone())
        .max_lines(1)
        .overflow(TextOverflow::Ellipsis)
        .style(
            TextStyle::new()
                .font_size(16.0)
                .font_weight(FontWeight::W500)
                .color(label),
        );
    let surface = BoxDecoration {
        color: Some(resolved(PILL_COLOR, app, context).into()),
        border_radius: Some(BorderRadiusGeometry::circular(PILL_RADIUS)),
        ..BoxDecoration::default()
    };
    DecoratedBox::new(surface)
        .child(
            Row::new()
                .main_axis_alignment(MainAxisAlignment::Center)
                .children([
                    program.icon(PILL_ICON_SIDE),
                    SizedBox::new().width(8.0).into_widget(),
                    waft::Flexible::new(name).into_widget(),
                ]),
        )
        .into_widget()
}

/// `child` on the bar's opaque surface, a hairline along its top.
pub(super) fn bar_material(child: WidgetRef, app: &mut App, context: BuildContext) -> WidgetRef {
    let hairline = resolved(CupertinoColors::SEPARATOR, app, context);
    let decoration = BoxDecoration {
        color: Some(resolved(BAR_COLOR, app, context).into()),
        border: Some(Box::new(top_hairline(hairline))),
        ..BoxDecoration::default()
    };
    DecoratedBox::new(decoration).child(child).into_widget()
}

/// One physical pixel along the top, as a navigation bar has along its bottom.
fn top_hairline(color: Color) -> Border {
    Border {
        top: BorderSide {
            color,
            width: 0.0,
            style: BorderStyle::Solid,
            stroke_align: BorderSide::STROKE_ALIGN_INSIDE,
        },
        right: BorderSide::NONE,
        bottom: BorderSide::NONE,
        left: BorderSide::NONE,
    }
}

fn tabs_button(on_pressed: Listener) -> WidgetRef {
    CupertinoButton::new(
        Icon::new(Some(CupertinoIcons::square_on_square()))
            .size(24.0)
            .into_widget(),
        Some(on_pressed),
    )
    .padding(EdgeInsetsGeometry::all(0.0))
    .minimum_size(Size::square(TABS_BUTTON_SIDE))
    .into_widget()
}
