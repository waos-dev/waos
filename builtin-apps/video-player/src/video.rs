//! The picture, letterboxed: as large as the window lets it be at the video's own aspect, centred
//! on black, the subtitles over it in the user's caption style, as Waft's `PlayerView` draws
//! them. A click on it plays or pauses.

use std::rc::Rc;

use waft_embedder::Color;
use waft_foundation::{App, Handle, Listenable, Listener};
use waft_rendering::HitTestBehavior;
use waft_widgets::{
    AspectRatio, Center, ColoredBox, GestureDetector, IntoWidget, ListenableBuilder, Player,
    PlayerView, WidgetRef,
};

use crate::commands;

/// `player`'s picture, letterboxed on black.
pub(crate) fn letterboxed(player: Handle<Player>) -> WidgetRef {
    let listenable: Rc<dyn Listenable> = Rc::new(player);
    let picture =
        ListenableBuilder::new(listenable, move |app, _context, _child| fitted(app, player));
    let toggle = Listener::new(move |app: &mut App| commands::play_or_pause(app, player));
    GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(toggle)
        .child(ColoredBox::new(BLACK).child(Center::new().child(picture)))
        .into_widget()
}

/// The `PlayerView`, made as large as the room allows at the video's aspect, since the view
/// alone grows no larger than the video's own size. It stays mounted while the size is unknown,
/// taking no room, so the video has its layer from the start.
fn fitted(app: &App, player: Handle<Player>) -> WidgetRef {
    let aspect = player
        .state(app)
        .size
        .filter(|size| size.width() > 0.0 && size.height() > 0.0)
        .map_or(UNKNOWN_ASPECT, |size| size.width() / size.height());
    AspectRatio::new(aspect)
        .child(PlayerView::new(player))
        .into_widget()
}

/// What letterboxes the picture.
const BLACK: Color = Color::from_argb(255, 0, 0, 0);

/// The aspect the picture's room keeps until the video's is known.
const UNKNOWN_ASPECT: f64 = 16.0 / 9.0;
