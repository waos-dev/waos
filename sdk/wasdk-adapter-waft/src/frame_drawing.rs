//! Whether a view's frame is drawn, decided by what its scene paints and places and by the frame
//! the view shows, as Waft's own hosts decide it from the scene's slices.
//!
//! A view's picture and its composition layers reach the screen together, in one present: the
//! layers lie beneath the picture, showing through the room it makes for each, so the room moves
//! only with a picture drawn. A frame is drawn when it paints, when the frame the view shows
//! painted, which must not stay, or when it places other layers, or the same ones elsewhere, than
//! that frame. A frame that paints nothing and places what the view shows where it shows it
//! changes no pixel: it is not drawn, and the view keeps showing what it showed. The invariant a
//! later editor must keep: a frame left undrawn looks the same as if it had been drawn.

use std::sync::Arc;

use waft_embedder::{CompositionLayer, Placement, Slices};

/// What a frame shows: whether it paints anything besides its composition layers, and the layers
/// it places, each with its placement, the bottom-most first. Held for the frame a view shows,
/// it keeps that frame's layers alive while they are shown.
#[derive(Default)]
pub(crate) struct ShownFrame {
    paints: bool,
    layers: Vec<(Arc<dyn CompositionLayer>, Placement)>,
}

impl ShownFrame {
    /// What the frame `slices` were made of shows.
    pub(crate) fn of(slices: &Slices) -> ShownFrame {
        ShownFrame {
            paints: slices.paints(),
            layers: slices
                .composition_layers()
                .map(|(layer, placement)| (Arc::clone(layer), placement.clone()))
                .collect(),
        }
    }

    /// The layers the frame places, each with its placement, the bottom-most first.
    pub(crate) fn layers(&self) -> &[(Arc<dyn CompositionLayer>, Placement)] {
        &self.layers
    }

    /// Whether this frame, drawn, changes what a view showing `shown` shows.
    pub(crate) fn changes(&self, shown: &ShownFrame) -> bool {
        self.paints || shown.paints || !self.places_as(shown)
    }

    /// Whether this frame places the layers `other` places, in the same order and places.
    fn places_as(&self, other: &ShownFrame) -> bool {
        self.layers.len() == other.layers.len()
            && self.layers.iter().zip(&other.layers).all(
                |((layer, placement), (other_layer, other_placement))| {
                    Arc::ptr_eq(layer, other_layer) && placement == other_placement
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::{HardwareBuffer, Rect};

    use super::*;

    /// A layer that shows nothing.
    struct Layer;

    impl CompositionLayer for Layer {
        fn present(
            &self,
            _buffer: Arc<dyn HardwareBuffer>,
            _children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
            _released: Box<dyn FnOnce() + Send + Sync>,
        ) {
        }

        fn current_picture(&self) -> Option<waft_embedder::LayerPicture> {
            None
        }
    }

    fn at(left: f64) -> Placement {
        Placement {
            rect: Rect::from_ltwh(left, 0.0, 100.0, 100.0),
            mutators: Vec::new(),
        }
    }

    fn frame(paints: bool, layers: &[(&Arc<dyn CompositionLayer>, f64)]) -> ShownFrame {
        ShownFrame {
            paints,
            layers: layers
                .iter()
                .map(|(layer, left)| (Arc::clone(layer), at(*left)))
                .collect(),
        }
    }

    #[test]
    fn a_frame_that_paints_is_drawn() {
        for shown in [frame(false, &[]), frame(true, &[])] {
            assert!(frame(true, &[]).changes(&shown));
        }
    }

    #[test]
    fn a_frame_that_paints_nothing_after_one_that_did_is_drawn_so_the_painting_goes() {
        assert!(frame(false, &[]).changes(&frame(true, &[])));
    }

    #[test]
    fn a_frame_that_paints_nothing_and_places_what_is_shown_is_left_undrawn() {
        let layer: Arc<dyn CompositionLayer> = Arc::new(Layer);
        assert!(!frame(false, &[]).changes(&ShownFrame::default()));
        assert!(!frame(false, &[(&layer, 0.0)]).changes(&frame(false, &[(&layer, 0.0)])));
    }

    /// The room for a layer is in the picture, so moving, adding or taking off a layer draws one.
    #[test]
    fn a_frame_that_places_its_layers_otherwise_is_drawn() {
        let video: Arc<dyn CompositionLayer> = Arc::new(Layer);
        let badge: Arc<dyn CompositionLayer> = Arc::new(Layer);
        let other: Arc<dyn CompositionLayer> = Arc::new(Layer);
        let shown = frame(false, &[(&video, 0.0), (&badge, 0.0)]);
        let moved = frame(false, &[(&video, 10.0), (&badge, 0.0)]);
        let taken_off = frame(false, &[(&badge, 0.0)]);
        let reordered = frame(false, &[(&badge, 0.0), (&video, 0.0)]);
        let another = frame(false, &[(&other, 0.0), (&badge, 0.0)]);
        for frame in [moved, taken_off, reordered, another] {
            assert!(frame.changes(&shown));
        }
    }
}
