//! The picture a view shows: the last one handed to the shell, with the layers handed with it,
//! beneath it, kept for whoever reads it, a tool that looks at the program's window among them
//! (the design's decisions 60 and 62), with how many of the view's events the program had read
//! when it presented it, so a reader waits for the first picture drawn after the program heard what
//! the view was told up to some moment.
//!
//! The invariant a later editor will break: the hardware buffer of the picture kept is held from
//! its pool as the shell's is, and goes back to the pool only once the next picture replaces it
//! and the shell has let go of it too, so its pixels are never drawn over while someone reads them;
//! and the layers kept are those the shell shows with the picture, not those set since, which wait
//! for the next, reached weakly, as the program's views reach its layers, so they go with the
//! program: one the engine no longer holds, the program having let go of it and set its view's
//! layers without it, is left out, though the shell shows it until the view's next picture.

use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::watch;

use crate::bindings::wasdk::platform::views::Placement;
use crate::hardware_buffers::Released;
use crate::os::HardwareBufferMemory;
use crate::shell::{HardwareBuffer, ShellCompositionLayer};
use crate::sync::locked;

use super::view_layers::PlacedShellLayer;

/// What a view shows, as its frames' thread hands pictures to the shell and readers read them.
pub(super) struct ShownPicture(watch::Sender<Showing>);

#[derive(Clone)]
enum Showing {
    /// No picture was handed to the shell yet.
    Nothing,
    Picture(Arc<Picture>),
    /// The view is gone, and shows nothing more.
    ViewGone,
}

/// One picture handed to the shell.
struct Picture {
    memory: HardwareBufferMemory,
    /// The layers it was handed with, the bottom-most first.
    layers: Vec<(Weak<dyn ShellCompositionLayer>, Placement)>,
    /// How many of the view's events the program had read when it presented the picture.
    events_read: u64,
    /// The pool's release of the picture's hardware buffer, which goes once the shell and this
    /// record have both let go of it.
    _held: Arc<ReleasedWhenDropped>,
}

/// A release run once whoever shares it has let go.
struct ReleasedWhenDropped(Mutex<Option<Released>>);

impl Drop for ReleasedWhenDropped {
    fn drop(&mut self) {
        if let Some(released) = locked(&self.0).take() {
            released();
        }
    }
}

/// The picture a view shows, as a reader holds it: its buffer is not drawn over meanwhile.
pub(super) struct Shown(Arc<Picture>);

impl Shown {
    /// The program's picture.
    pub(super) fn drawing(&self) -> &HardwareBufferMemory {
        &self.0.memory
    }

    /// The layers beneath it the engine holds still, the bottom-most first, each where the program
    /// placed it.
    pub(super) fn layers(&self) -> Vec<PlacedShellLayer> {
        self.0
            .layers
            .iter()
            .filter_map(|(layer, placement)| Some((layer.upgrade()?, placement.clone())))
            .collect()
    }
}

/// A picture on its way to the shell, which the view shows once the shell has it.
pub(super) struct HandedPicture(Arc<Picture>);

impl HandedPicture {
    /// `buffer`, presented with `layers` beneath it after the program had read `events_read` of
    /// the view's events, with `released`, the pool's release of it: the picture, and the release
    /// the shell is handed with it, which gives the buffer back to its pool once the next picture
    /// has replaced this one too.
    pub(super) fn new(
        buffer: &HardwareBuffer,
        layers: Vec<PlacedShellLayer>,
        released: Released,
        events_read: u64,
    ) -> (HandedPicture, Released) {
        let held = Arc::new(ReleasedWhenDropped(Mutex::new(Some(released))));
        let layers = layers
            .into_iter()
            .map(|(layer, placement)| (Arc::downgrade(&layer), placement))
            .collect();
        let picture = HandedPicture(Arc::new(Picture {
            memory: HardwareBufferMemory::of(buffer.clone()),
            layers,
            events_read,
            _held: Arc::clone(&held),
        }));
        (picture, Box::new(move || drop(held)))
    }
}

impl ShownPicture {
    pub(super) fn new() -> ShownPicture {
        ShownPicture(watch::Sender::new(Showing::Nothing))
    }

    /// The shell has `picture`: the view shows it from now on, in place of the one before. A view
    /// gone keeps nothing, and the shell's release alone counts.
    pub(super) fn show(&self, picture: HandedPicture) {
        let mut replaced = None;
        self.0.send_if_modified(|showing| {
            if matches!(showing, Showing::ViewGone) {
                return false;
            }
            replaced = Some(std::mem::replace(showing, Showing::Picture(picture.0)));
            true
        });
        // The picture before's release runs here, outside the lock the watch holds.
        drop(replaced);
    }

    /// The view is gone: it shows nothing more, its last picture goes back to its pool once the
    /// shell lets go of it, and nobody waits for another.
    pub(super) fn end(&self) {
        let last = self.0.send_replace(Showing::ViewGone);
        // The picture's release runs here, outside the lock the watch holds.
        drop(last);
    }

    /// The picture shown now and the layers beneath it, held for as long as what is answered is;
    /// none before the first, and none once the view is gone.
    pub(super) fn now(&self) -> Option<Shown> {
        match &*self.0.borrow() {
            Showing::Picture(picture) => Some(Shown(Arc::clone(picture))),
            Showing::Nothing | Showing::ViewGone => None,
        }
    }

    /// Resolves with true once the view shows a picture the program presented after reading
    /// `events` of the view's events, at once for the one shown now if it was; with false once
    /// the view is gone first.
    pub(super) fn after_events(
        &self,
        events: u64,
    ) -> impl Future<Output = bool> + Send + 'static + use<> {
        let mut showing = self.0.subscribe();
        async move {
            loop {
                let answered = match &*showing.borrow_and_update() {
                    Showing::Picture(picture) if picture.events_read >= events => Some(true),
                    Showing::ViewGone => Some(false),
                    Showing::Picture(_) | Showing::Nothing => None,
                };
                if let Some(answered) = answered {
                    return answered;
                }
                if showing.changed().await.is_err() {
                    return false;
                }
            }
        }
    }
}

#[cfg(all(test, target_vendor = "apple"))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::FutureExt;

    use super::*;
    use crate::bindings::wasdk::platform::hardware_buffers::HardwareBufferUsage;

    /// A picture's hardware buffer, two pixels wide.
    fn buffer() -> HardwareBuffer {
        HardwareBufferMemory::new(2, 1, HardwareBufferUsage::empty())
            .unwrap()
            .buffer()
    }

    /// A pool's release that counts how often it ran.
    fn counted(releases: &Arc<AtomicUsize>) -> Released {
        let releases = Arc::clone(releases);
        Box::new(move || {
            releases.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// `buffer` shown, as the shell takes it; the release the shell is handed.
    fn shown_with(shown: &ShownPicture, released: Released, events_read: u64) -> Released {
        let (picture, shells) = HandedPicture::new(&buffer(), Vec::new(), released, events_read);
        shown.show(picture);
        shells
    }

    fn nothing() -> Released {
        Box::new(|| {})
    }

    #[test]
    fn a_buffer_goes_back_to_its_pool_once_both_the_shell_and_the_next_picture_let_go() {
        let shown = ShownPicture::new();
        let releases = Arc::new(AtomicUsize::new(0));
        let shells = shown_with(&shown, counted(&releases), 0);
        shells();
        assert_eq!(
            releases.load(Ordering::SeqCst),
            0,
            "the picture is still shown"
        );
        let next = shown_with(&shown, counted(&releases), 1);
        assert_eq!(releases.load(Ordering::SeqCst), 1, "replaced and let go");
        drop(next);
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "the shell's letting go alone keeps the picture shown"
        );
        shown.end();
        assert_eq!(
            releases.load(Ordering::SeqCst),
            2,
            "the view gone keeps none"
        );
    }

    #[test]
    fn what_is_shown_is_the_last_picture_with_its_layers_and_none_before_the_first_or_after_the_view()
     {
        let shown = ShownPicture::new();
        assert!(shown.now().is_none());
        let layer: Arc<dyn crate::shell::ShellCompositionLayer> = Arc::new(Layer);
        let layers = vec![(Arc::clone(&layer), placement())];
        let (picture, _shells) = HandedPicture::new(&buffer(), layers, nothing(), 0);
        assert!(shown.now().is_none(), "on its way, not yet shown");
        shown.show(picture);
        let now = shown.now().expect("a picture");
        assert_eq!((now.drawing().width(), now.drawing().height()), (2, 1));
        let layers = now.layers();
        assert_eq!(layers.len(), 1);
        assert!(Arc::ptr_eq(&layers[0].0, &layer));
        drop(layers);
        assert_eq!(Arc::strong_count(&layer), 1, "the picture holds none");
        drop(layer);
        assert!(now.layers().is_empty(), "a layer let go of is left out");
        drop(now);
        shown.end();
        assert!(shown.now().is_none());
    }

    struct Layer;

    impl crate::shell::ShellCompositionLayer for Layer {
        fn present(&self, _buffer: HardwareBuffer, released: Released) {
            released();
        }

        fn current_picture(&self) -> Option<crate::shell::LayerPicture> {
            None
        }
    }

    fn placement() -> crate::bindings::wasdk::platform::views::Placement {
        use crate::bindings::wasdk::platform::windows::{Point, Rect, Size};

        crate::bindings::wasdk::platform::views::Placement {
            rect: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 1.0,
                    height: 1.0,
                },
            },
            mutators: Vec::new(),
        }
    }

    #[test]
    fn a_wait_ends_at_the_first_picture_presented_after_the_events_were_read() {
        let shown = ShownPicture::new();
        let _first = shown_with(&shown, nothing(), 3);
        assert_eq!(shown.after_events(3).now_or_never(), Some(true));
        let mut waiting = Box::pin(shown.after_events(5));
        assert_eq!((&mut waiting).now_or_never(), None);
        let _before = shown_with(&shown, nothing(), 4);
        assert_eq!(
            (&mut waiting).now_or_never(),
            None,
            "drawn before the events"
        );
        let _after = shown_with(&shown, nothing(), 5);
        assert_eq!(waiting.now_or_never(), Some(true));
    }

    #[test]
    fn a_wait_ends_without_a_picture_when_the_view_goes() {
        let shown = ShownPicture::new();
        let mut waiting = Box::pin(shown.after_events(1));
        assert_eq!((&mut waiting).now_or_never(), None);
        shown.end();
        assert_eq!(waiting.now_or_never(), Some(false));
    }
}
