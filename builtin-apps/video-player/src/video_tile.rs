//! A video's tile in the library's grid: its thumbnail, with a bar along its bottom for how far
//! it was watched, its title beneath, and its length; a click plays it.

use waft_embedder::Color;
use waft_foundation::{App, Entity, Listener};
use waft_rendering::HitTestBehavior;
use waft_widgets::{
    AlignmentGeometry, AspectRatio, BorderRadiusGeometry, ClipRRect, ColoredBox, Column,
    CrossAxisAlignment, FractionallySizedBox, GestureDetector, IntoWidget, Positioned, SizedBox,
    Stack, StackFit, WidgetRef,
};
use waft_winui::TextBlockStyle;

use crate::clock::clock;
use crate::library::Tile;
use crate::playback::{self, Playback};
use crate::text::{Tone, line};
use crate::thumbnail::thumbnail;

/// A thumbnail's shape, a film's.
pub(crate) const THUMBNAIL_ASPECT: f64 = 16.0 / 9.0;

/// How tall the title and the length beneath a thumbnail are, with the room above them.
pub(crate) const CAPTION_HEIGHT: f64 = 48.0;

/// How tall the bar of how far a video was watched is.
const WATCHED_BAR_HEIGHT: f64 = 4.0;

/// The bar's part watched, and the part not watched yet: the picture darkened.
const WATCHED: Color = Color::from_argb(255, 255, 255, 255);
const NOT_WATCHED: Color = Color::from_argb(150, 0, 0, 0);

/// `tile`'s video, which a click plays in `playback`.
pub(crate) fn video_tile(playback: &Entity<Playback>, tile: Tile) -> WidgetRef {
    let (playback, source) = (playback.clone(), tile.source.clone());
    let play = Listener::new(move |app: &mut App| playback::open(&playback, app, source.clone()));
    let mut picture = vec![thumbnail(tile.thumbnail)];
    if let Some(watched) = tile.watched {
        picture.push(watched_bar(watched));
    }
    let thumbnail = ClipRRect::new()
        .border_radius(BorderRadiusGeometry::circular(8.0))
        .child(
            AspectRatio::new(THUMBNAIL_ASPECT)
                .child(Stack::new().fit(StackFit::Expand).children(picture)),
        );
    let length = tile.duration.map(clock).unwrap_or_default();
    let caption = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([
            SizedBox::new().height(6.0).into_widget(),
            line(tile.title, TextBlockStyle::Body, Tone::Primary),
            line(length, TextBlockStyle::Caption, Tone::Secondary),
        ]);
    GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(play)
        .child(
            Column::new()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children([thumbnail.into_widget(), caption.into_widget()]),
        )
        .into_widget()
}

/// The bar along a thumbnail's bottom, filled `watched` of the way, from 0 to 1.
fn watched_bar(watched: f64) -> WidgetRef {
    let filled = FractionallySizedBox::new()
        .alignment(AlignmentGeometry::CENTER_LEFT)
        .width_factor(watched)
        .child(ColoredBox::new(WATCHED));
    let bar = ColoredBox::new(NOT_WATCHED).child(filled);
    Positioned::new(SizedBox::new().height(WATCHED_BAR_HEIGHT).child(bar))
        .left(0.0)
        .right(0.0)
        .bottom(0.0)
        .into_widget()
}
