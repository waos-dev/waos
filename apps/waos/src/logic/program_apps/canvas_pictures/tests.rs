use futures::channel::mpsc::{self, UnboundedReceiver};
use waos::bindings::wasdk::platform::views::Insets;
use waos::bindings::wasdk::platform::windows::WindowBackground;
use waos_window_messages::os::macos::ReceiveRight;

use super::super::canvases::CanvasCall;
use super::super::window_process::tests::Compositing;
use super::*;
use crate::os::test_buffer;

const WINDOW: RequestId = 3;
const OPENED: PhysicalSize = [800, 600];
const RESIZED: PhysicalSize = [1000, 600];

fn metrics([width, height]: PhysicalSize) -> Metrics {
    let none = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    Metrics {
        width,
        height,
        device_pixel_ratio: 2.0,
        padding: none,
        view_padding: none,
        view_insets: none,
    }
}

/// A fence, as a window sends one with a size it waits at.
fn fence() -> SendRight {
    ReceiveRight::new().unwrap().make_send().unwrap()
}

/// The canvas of a view in a program's app, what the window is told, and the calls on the main
/// thread.
struct Shown {
    pictures: Arc<CanvasPictures>,
    told: UnboundedReceiver<ToWindow>,
    calls: UnboundedReceiver<CanvasCall>,
}

fn canvas() -> Shown {
    let (told_window, told) = mpsc::unbounded();
    let send: SendToWindow = Arc::new(move |message| {
        let _ = told_window.unbounded_send(message);
    });
    let (canvases, calls) = CanvasesOnRequest::new();
    let layer = Arc::new(CanvasLayer::make(&Compositing, WindowBackground::Solid));
    let pictures = CanvasPictures::new(WINDOW, layer, Arc::new(canvases), 1, send);
    Shown {
        pictures,
        told,
        calls,
    }
}

impl Shown {
    /// The program presents a picture of `[width, height]`.
    fn present(&self, [width, height]: PhysicalSize) {
        let buffer = test_buffer(width as usize, height as usize);
        self.pictures.present(buffer, Vec::new(), Box::new(|| {}));
    }

    /// The size of the picture the window is told its view shows next; none when it is told
    /// nothing.
    fn told(&mut self) -> Option<PhysicalSize> {
        match self.told.try_recv().ok()? {
            ToWindow::PictureShown { window, size } => {
                assert_eq!(window, WINDOW);
                Some(size)
            }
            other => panic!("the window is told only of pictures: {other:?}"),
        }
    }

    /// The next call on the main thread; none when nothing went there.
    fn call(&mut self) -> Option<CanvasCall> {
        self.calls.try_recv().ok()
    }

    /// Carries out the next call on the main thread, which must be one to show, as the main
    /// thread does with no window server: the picture is shown, and the canvas hears so. Answers
    /// the size of the resize it answered.
    fn show_on_main_thread(&mut self) -> Option<PhysicalSize> {
        let Some(CanvasCall::Show {
            shown, pictures, ..
        }) = self.call()
        else {
            panic!("something to show went to the main thread");
        };
        let answered = shown.answered();
        match shown {
            ToShow::Picture(picture) | ToShow::Resize(_, Some(picture)) => pictures.show(picture),
            ToShow::Resize(_, None) => {}
        }
        pictures.shown_on_main_thread(answered);
        answered
    }
}

#[test]
fn the_window_hears_of_its_views_first_picture_alone_until_it_waits() {
    let mut shown = canvas();
    shown.pictures.resize_at_once(metrics(OPENED), None);
    assert!(matches!(
        shown.call(),
        Some(CanvasCall::Place { fence: None, .. })
    ));
    shown.present(OPENED);
    assert_eq!(shown.told(), Some(OPENED), "the first picture");
    shown.present(OPENED);
    shown.present(RESIZED);
    assert_eq!(shown.told(), None, "and no other, while nothing waits");
    assert!(shown.call().is_none(), "each shown where it was presented");
}

#[test]
fn nothing_waits_before_a_views_first_picture() {
    let mut shown = canvas();
    shown.pictures.resize(metrics(RESIZED), fence());
    assert!(
        matches!(shown.call(), Some(CanvasCall::Place { fence: Some(_), .. })),
        "the canvas is placed at once, and the fence goes with the layout"
    );
    assert_eq!(shown.told(), None);
}

#[test]
fn a_resize_is_answered_by_the_picture_of_its_size_alone_shown_with_the_layout() {
    let mut shown = canvas();
    shown.present(OPENED);
    assert_eq!(shown.told(), Some(OPENED));

    shown.pictures.resize(metrics(RESIZED), fence());
    assert!(
        shown.call().is_none(),
        "the fence is kept until the picture"
    );
    shown.present(OPENED);
    shown.present([1000, 601]);
    assert!(
        shown.call().is_none(),
        "pictures of other sizes show at once"
    );
    assert_eq!(shown.told(), None, "and answer nothing");

    shown.present(RESIZED);
    assert_eq!(shown.told(), None, "not before the main thread showed it");
    shown.present(RESIZED);
    assert_eq!(
        shown.show_on_main_thread(),
        Some(RESIZED),
        "the picture of the size waited for is shown with the layout"
    );
    assert_eq!(shown.told(), Some(RESIZED));
    assert_eq!(
        shown.show_on_main_thread(),
        None,
        "the picture after it follows it there, so none overtakes it"
    );
    assert_eq!(shown.told(), None);

    shown.present(RESIZED);
    assert!(
        shown.call().is_none(),
        "once those are shown, pictures show at once again"
    );
}

#[test]
fn a_resize_to_the_size_shown_already_is_answered_as_it_is_laid_out() {
    let mut shown = canvas();
    shown.present(OPENED);
    shown.present(RESIZED);
    assert_eq!(shown.told(), Some(OPENED));
    shown.pictures.resize(metrics(RESIZED), fence());
    assert_eq!(shown.show_on_main_thread(), Some(RESIZED));
    assert_eq!(shown.told(), Some(RESIZED));
}

#[test]
fn a_new_size_reported_without_a_fence_waits_for_nothing() {
    let mut shown = canvas();
    shown.present(OPENED);
    assert_eq!(shown.told(), Some(OPENED));
    shown.pictures.resize_at_once(metrics(RESIZED), None);
    assert!(matches!(
        shown.call(),
        Some(CanvasCall::Place { fence: None, .. })
    ));
    shown.present(RESIZED);
    assert!(shown.call().is_none());
    assert_eq!(shown.told(), None);
}
