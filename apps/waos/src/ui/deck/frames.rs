//! Each tab drawn where the geometry places it: its page, laid out at the page's size in its place
//! on the strip, and drawn moved and scaled from there into its place and clipped, with the card's
//! shadow beneath; and after it what the overview draws around its card (`card_chrome`).
//!
//! Every tab's widgets have the same shape whatever the deck is doing, and are keyed by the tab,
//! so moving between the strip and the overview's grid, or being painted in another order, never
//! builds a page again or moves it in the tree. A tab that cannot be seen is offstage and its
//! tickers are muted, so a program's canvas there is not placed.
//!
//! A page's flight is drawn by a transform that leaves touches where the page is laid out, as
//! Flutter's `Transform` with `transformHitTests: false` and as iOS hit-tests a view's final
//! frame rather than what an animation draws: a touch reaches a page, while it takes touches, at
//! the place it has once its flight lands on the strip. Its clip is inside the transform, in the
//! page's own points, since a clip limits where touches land in Waft as in Flutter, and Flutter
//! has no clip that cuts only the drawing: outside the transform it would limit them to where the
//! clip is drawn. So what the clip cuts away of a page takes no touch until it shows.
//!
//! A page's clip is in the tree always and clips only while the geometry says it cuts something:
//! a page on the strip fills its clip with square corners, and a clip that cuts nothing still
//! costs the renderer a pass over the whole picture.
//!
//! The invariant: nothing between the deck and a program's page fades it or changes its size. A
//! program's picture shows through a clear hole in WaOS's picture, and an opacity above the hole
//! clears nothing; only the home's page, which is WaOS's own drawing, is drawn through an
//! opacity, for the page past the last tab.

use std::any::Any;

use waft::ui::{Matrix4, RRect, Radius};
use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BoxShadow, BuildContext, Clip, ClipRRect, Color,
    DecoratedBox, Handle, IgnorePointer, IntoWidget, KeyedSubtree, MediaQuery, MediaQueryData,
    Offset, Offstage, Opacity, Positioned, Size, Stack, TickerMode, Transform, ValueKey, WidgetRef,
};
use waft_rendering::CustomClipper;

use super::card_chrome::card_chrome;
use super::{DeckViewState, FrameKey, ShownTab};
use crate::logic::deck_geometry::{CARD_RADIUS, DeckLayout, TabPlacement};

/// A tab, placed for this frame.
pub(super) struct PlacedTab<'a> {
    pub(super) tab: &'a ShownTab,
    pub(super) placement: TabPlacement,
}

impl DeckViewState {
    /// Every tab's page in its frame, each followed by what shows around its card, in paint
    /// order. `page_media` is what a page is told of the box it is laid out in.
    pub(super) fn tab_frames(
        self: Handle<Self>,
        app: &mut App,
        context: BuildContext,
        placed: &[PlacedTab],
        page_media: &MediaQueryData,
    ) -> Vec<WidgetRef> {
        let layout = self.layout(app);
        let mut frames = Vec::new();
        for tab in self.in_paint_order(app, placed) {
            frames.push(self.page_frame(app, tab, &layout, page_media));
            frames.extend(card_chrome(tab, app, context));
        }
        frames
    }

    fn page_frame(
        self: Handle<Self>,
        app: &App,
        placed: &PlacedTab,
        layout: &DeckLayout,
        page_media: &MediaQueryData,
    ) -> WidgetRef {
        let placement = placed.placement;
        let key = placed.tab.key;
        let page = app
            .get(self)
            .pages
            .get(&key)
            .map(|page| page.widget.clone())
            .expect("every tab drawn has a page");
        let told = MediaQuery::new(
            page_media.clone(),
            IgnorePointer::new()
                .ignoring(!self.page_takes_touches(app, placed.tab.tab))
                .child(page),
        );
        let frame = Stack::new().clip_behavior(Clip::None).children([
            card_shadow(&placement),
            flown_and_clipped(told.into_widget(), &placement, layout),
        ]);
        let drawn = match key {
            FrameKey::Home => Opacity::new(placement.opacity).child(frame).into_widget(),
            FrameKey::Program(_) => frame.into_widget(),
        };
        let visible = placement.is_visible(layout);
        KeyedSubtree::new(TickerMode::new(
            visible,
            Offstage::new().offstage(!visible).child(drawn),
        ))
        .key(ValueKey::new(key).into())
        .into_widget()
    }
}

/// `page`, laid out in `placement`'s place for it on the strip, and drawn moved and scaled from
/// there into its page by a transform that leaves touches where the page is laid out, clipped to
/// its clip in the page's own points. The clip stays where it is in the tree and clips only while
/// it cuts something of the page: a clip costs the renderer a pass over the whole picture,
/// whatever it cuts.
fn flown_and_clipped(page: WidgetRef, placement: &TabPlacement, layout: &DeckLayout) -> WidgetRef {
    let scale = placement.scale(layout) as f32;
    let moved = placement.drawn_offset();
    let flight = Matrix4::translation(moved.dx() as f32, moved.dy() as f32)
        .then(&Matrix4::scale(scale, scale));
    let shown = RRect::from_rect_and_radius(
        placement.clip_on_the_page(layout),
        Radius::circular(placement.corner_radius_on_the_page(layout).max(0.0)),
    );
    let clip_behavior = if placement.clips() {
        Clip::AntiAlias
    } else {
        Clip::None
    };
    let clipped = ClipRRect::new()
        .clipper(ShownPart(shown))
        .clip_behavior(clip_behavior)
        .child(page);
    Positioned::from_rect(
        placement.laid_out,
        Transform::new(flight)
            .transform_hit_tests(false)
            .child(clipped),
    )
    .into_widget()
}

/// What of a page shows, in the page's own points.
#[derive(Debug, PartialEq)]
struct ShownPart(RRect);

impl CustomClipper<RRect> for ShownPart {
    fn get_clip(&self, _size: Size) -> RRect {
        self.0
    }

    fn should_reclip(&self, old_clipper: &dyn CustomClipper<RRect>) -> bool {
        old_clipper
            .as_any()
            .downcast_ref::<ShownPart>()
            .is_none_or(|old| old != self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A card's lift off the overview's background, under its clip where it is drawn, growing in with
/// its corners as the overview opens; none on a page. It takes no touch.
fn card_shadow(placement: &TabPlacement) -> WidgetRef {
    let radius = BorderRadiusGeometry::circular(placement.corner_radius.max(0.0));
    let lifted = BoxDecoration {
        border_radius: Some(radius),
        box_shadow: card_shadow_of(placement.corner_radius),
        ..BoxDecoration::default()
    };
    Positioned::from_rect(
        placement.clip,
        IgnorePointer::new().child(DecoratedBox::new(lifted)),
    )
    .into_widget()
}

/// The shadow of a card whose corners are `corner_radius` round.
fn card_shadow_of(corner_radius: f64) -> Option<Vec<BoxShadow>> {
    let strength = (corner_radius / CARD_RADIUS).clamp(0.0, 1.0);
    if strength == 0.0 {
        return None;
    }
    Some(vec![BoxShadow::new(
        Color::from_argb((40.0 * strength) as i32, 0, 0, 0),
        Offset::new(0.0, 4.0),
        14.0,
        0.0,
        waft::BlurStyle::Normal,
    )])
}
