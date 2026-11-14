//! The layers each of a program's views holds beneath its picture, as the program last set them
//! with `set-layers`, which each present of the view hands the shell with its picture. The
//! program's store keeps them, as it keeps the program's handles on the layers, so they go with the
//! program; the program's views reach them weakly, so a view the shell takes away lets go of its
//! layers then. A view's are kept until the program sets them again or the view goes.
//!
//! The invariants: a view's layers are replaced whole, never merged, so a layer the program's list
//! leaves out is taken off with the view's next picture; and a view gone holds none, whatever the
//! program sets on it after, since ids are never reused.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::bindings::wasdk::platform::views::{Placement, ViewId};
use crate::shell::ShellCompositionLayer;
use crate::sync::locked;

/// A layer as the shell is handed it: the shell's own, and where it goes.
pub(crate) type PlacedShellLayer = (Arc<dyn ShellCompositionLayer>, Placement);

/// The layers each of a program's views holds, by view; a view the program set none on holds
/// none.
#[derive(Default)]
pub(crate) struct ViewLayers(Mutex<Held>);

#[derive(Default)]
struct Held {
    layers: HashMap<ViewId, Vec<PlacedShellLayer>>,
    /// The views the shell took away.
    gone: HashSet<ViewId>,
}

impl ViewLayers {
    /// `view` holds `layers` from now on, the bottom-most first, in place of what it held; a view
    /// gone holds nothing.
    pub(crate) fn set(&self, view: ViewId, layers: Vec<PlacedShellLayer>) {
        let mut held = locked(&self.0);
        if layers.is_empty() || held.gone.contains(&view) {
            held.layers.remove(&view);
        } else {
            held.layers.insert(view, layers);
        }
    }

    /// The layers `view` holds, for its next present.
    pub(crate) fn of(&self, view: ViewId) -> Vec<PlacedShellLayer> {
        let held = locked(&self.0);
        held.layers.get(&view).cloned().unwrap_or_default()
    }

    /// The shell took `view` away: it lets go of its layers, and holds none from now on.
    pub(crate) fn view_removed(&self, view: ViewId) {
        let released = {
            let mut held = locked(&self.0);
            held.gone.insert(view);
            held.layers.remove(&view)
        };
        // The shell's layers go outside the lock, whatever their own drop does.
        drop(released);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::wasdk::platform::windows::{Point, Rect, Size};
    use crate::shell::HardwareBuffer;

    struct Layer;

    impl ShellCompositionLayer for Layer {
        fn present(&self, _buffer: HardwareBuffer, released: Box<dyn FnOnce() + Send + Sync>) {
            released();
        }

        fn current_picture(&self) -> Option<crate::shell::LayerPicture> {
            None
        }
    }

    fn at(x: f64) -> Placement {
        Placement {
            rect: Rect {
                origin: Point { x, y: 0.0 },
                size: Size {
                    width: 1.0,
                    height: 1.0,
                },
            },
            mutators: Vec::new(),
        }
    }

    /// Each layer of `layers` by where it sits, in order.
    fn positions(layers: &[PlacedShellLayer]) -> Vec<f64> {
        layers
            .iter()
            .map(|(_, placement)| placement.rect.origin.x)
            .collect()
    }

    #[test]
    fn a_view_holds_what_was_set_last_whole() {
        let layer: Arc<dyn ShellCompositionLayer> = Arc::new(Layer);
        let layers = ViewLayers::default();
        layers.set(
            1,
            vec![(Arc::clone(&layer), at(0.0)), (Arc::clone(&layer), at(1.0))],
        );
        layers.set(1, vec![(Arc::clone(&layer), at(2.0))]);
        assert_eq!(
            positions(&layers.of(1)),
            [2.0],
            "the first list is taken off whole"
        );
        assert!(layers.of(2).is_empty(), "another view holds none");
    }

    #[test]
    fn a_view_set_no_layers_lets_go_of_those_it_held() {
        let layer: Arc<dyn ShellCompositionLayer> = Arc::new(Layer);
        let layers = ViewLayers::default();
        layers.set(1, vec![(Arc::clone(&layer), at(0.0))]);
        layers.set(1, Vec::new());
        assert!(layers.of(1).is_empty());
        assert_eq!(
            Arc::strong_count(&layer),
            1,
            "nothing of the view's holds the layer"
        );
    }

    #[test]
    fn a_view_taken_away_lets_go_of_its_layers_and_holds_none_set_after() {
        let layer: Arc<dyn ShellCompositionLayer> = Arc::new(Layer);
        let layers = ViewLayers::default();
        layers.set(1, vec![(Arc::clone(&layer), at(0.0))]);
        layers.set(2, vec![(Arc::clone(&layer), at(1.0))]);
        layers.view_removed(1);
        assert!(layers.of(1).is_empty());
        assert_eq!(
            Arc::strong_count(&layer),
            2,
            "the other view holds it still"
        );
        layers.set(1, vec![(Arc::clone(&layer), at(2.0))]);
        assert!(
            layers.of(1).is_empty(),
            "a view gone holds no layers set after"
        );
        assert_eq!(Arc::strong_count(&layer), 2);
    }
}
