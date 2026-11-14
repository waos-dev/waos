//! The pictures a program presents to the canvas of a view in its app, and the resize its window
//! waits in for one (decision 55 of `docs/proposals/2026-10-17-the-waos-project.md`). A picture
//! goes to the canvas from the frame's thread, as any view's does. A new size the window sends
//! with a fence is a resize it waits in, inside AppKit's step, until WaOS says the program's
//! picture at that size is shown: WaOS keeps the fence until the program presents a picture of
//! that size, by size alone, as Flutter's macOS embedder matches a frame to its resize
//! (`ResizeSynchronizer.swift`), and then that picture goes to the main thread, where the canvas
//! is laid out at the new size with it in one transaction under the fence; then the window hears
//! that the picture is shown ([`ToWindow::PictureShown`]), and lets the resize go on. A resize to
//! the size of the picture the canvas shows already is laid out at once under its fence, and
//! answered the same way. Keeping the fence holds no window up, since the window server holds only
//! commits that carry one (`../experiments/2026-11-05-ca-fence-hold/`): the window commits the
//! transaction carrying it only once it stops waiting, so WaOS's commit under it comes first and is
//! held only until the window's, which lets the fence go, and both land together. Nothing waits
//! before a view's first picture, so a fence before it goes at once with the layout, and the window
//! hears of the first picture as it is shown, after which it waits in each resize. Nor does anything
//! wait while the program awaits the app's answer to a request WaOS made of it, since the program
//! may wait for that answer before it draws: the window's wait ends as such a request arrives, and
//! WaOS lets the fence go then, laying the canvas out at the size awaited at once
//! (`window_process`).
//!
//! The invariant a later editor will break: the canvas shows the pictures in the order the program
//! presented them. Once a picture goes to the main thread, every one after it goes there too until
//! those there are shown, so none overtakes it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use waos::bindings::wasdk::platform::views::{Metrics, Placement, RequestId};
use waos::{HardwareBuffer, ShellCompositionLayer};
use waos_program_window::{Canvas, CanvasLayer};
use waos_window_messages::ToWindow;
use waos_window_messages::os::macos::SendRight;

use super::canvases::{CanvasKey, CanvasesOnRequest};
use super::window_process::SendToWindow;
use crate::os::CrossThreadBuffer;

/// A view's size in physical pixels: its width and height.
pub(crate) type PhysicalSize = [u32; 2];

/// The canvas of a view in a program's app, as the program's pictures reach it.
pub(crate) struct CanvasPictures {
    me: Weak<CanvasPictures>,
    /// The window the view is in, by the request it was made for.
    window: RequestId,
    canvas: Arc<CanvasLayer>,
    /// Where the canvas is laid out, on the main thread, and the key it goes by there.
    canvases: Arc<CanvasesOnRequest>,
    key: CanvasKey,
    send: SendToWindow,
    showing: Mutex<Showing>,
}

/// A picture the program presented: its hardware buffer, the program's layers beneath it, and
/// what to call once the compositor is done with the buffer.
pub(crate) struct Picture {
    buffer: CrossThreadBuffer,
    layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
    released: Box<dyn FnOnce() + Send + Sync>,
}

/// A resize its window waits in for the program's picture at its size: the view's metrics at it,
/// and the fence the window's transaction committing that size waits for.
pub(crate) struct AwaitedResize {
    pub(crate) metrics: Metrics,
    pub(crate) fence: SendRight,
}

/// What the main thread shows on a canvas, in its turn.
pub(crate) enum ToShow {
    /// A picture the program presented, after one that went there before it.
    Picture(Picture),
    /// What answers the resize the window waits in: the canvas laid out at its size under the
    /// window's fence, with the program's picture at that size in the same transaction; none when
    /// the canvas shows one of that size already.
    Resize(AwaitedResize, Option<Picture>),
}

/// What a canvas knows of the pictures presented to it and of its window's resize, from every
/// thread that reaches it.
#[derive(Default)]
struct Showing {
    /// The size of the picture presented last, shown or on its way to the main thread; none before
    /// the first.
    last: Option<PhysicalSize>,
    /// The resize the window waits in, until a picture of its size.
    awaited: Option<AwaitedResize>,
    /// What went to the main thread and is not shown yet.
    on_main_thread: usize,
}

/// Where one picture goes.
enum Route {
    /// To the canvas, at once; the view's first, which the window hears of, when `first`.
    Canvas { first: bool },
    /// To the main thread, after what went there before it.
    MainThread,
    /// To the main thread, answering the resize the window waits in.
    Answer(AwaitedResize),
}

/// What a new size the window waits at does to the canvas.
enum Placing {
    /// The canvas is placed at it at once, under the fence: the view shows no picture yet.
    Now(SendRight),
    /// The canvas shows a picture of that size already: it is laid out at it under the fence, and
    /// the window waiting hears so.
    Answered(AwaitedResize),
    /// The canvas is laid out at it with the program's picture of that size, which the window
    /// waits for.
    Awaited,
}

impl Picture {
    pub(crate) fn new(
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) -> Picture {
        Picture {
            buffer: CrossThreadBuffer::new(buffer),
            layers,
            released,
        }
    }

    /// The size the program drew it at.
    fn size(&self) -> PhysicalSize {
        self.buffer.size()
    }
}

impl CanvasPictures {
    /// The canvas of the view in the window made for `window`, kept on the main thread by
    /// `canvases` under `key`, which tells the window through `send` what it shows.
    pub(crate) fn new(
        window: RequestId,
        canvas: Arc<CanvasLayer>,
        canvases: Arc<CanvasesOnRequest>,
        key: CanvasKey,
        send: SendToWindow,
    ) -> Arc<CanvasPictures> {
        Arc::new_cyclic(|me| CanvasPictures {
            me: me.clone(),
            window,
            canvas,
            canvases,
            key,
            send,
            showing: Mutex::default(),
        })
    }

    /// Resizes the canvas to the view's `metrics`, as its window reports them, with the program's
    /// picture at that size, which the window waits for in its resize, having sent `fence`.
    pub(crate) fn resize(&self, metrics: Metrics, fence: SendRight) {
        let placing = self.showing().resized(metrics, fence);
        match placing {
            Placing::Now(fence) => self.canvases.place(self.key, metrics, Some(fence)),
            Placing::Answered(resize) => self.show_on_main_thread(ToShow::Resize(resize, None)),
            Placing::Awaited => {}
        }
    }

    /// Resizes the canvas to the view's `metrics` at once, under `fence` when one came, the window
    /// waiting for no picture at that size.
    pub(crate) fn resize_at_once(&self, metrics: Metrics, fence: Option<SendRight>) {
        self.showing().awaited = None;
        self.canvases.place(self.key, metrics, fence);
    }

    /// The window waits in its resize no more: the canvas is laid out at the size awaited at once,
    /// under the window's fence, which goes with the layout.
    pub(crate) fn let_resize_go(&self) {
        let awaited = self.showing().awaited.take();
        if let Some(resize) = awaited {
            self.canvases
                .place(self.key, resize.metrics, Some(resize.fence));
        }
    }

    /// The window is gone: a resize it waited in lets its fence go, and nothing is shown again.
    pub(crate) fn window_gone(&self) {
        self.showing().awaited = None;
        self.canvas.window_gone();
    }

    /// Shows `picture` on the canvas, on the thread that calls.
    pub(crate) fn show(&self, picture: Picture) {
        let Picture {
            buffer,
            layers,
            released,
        } = picture;
        self.canvas.present(buffer.into_buffer(), layers, released);
    }

    /// The main thread showed what went to it; the window waiting in its resize to `answered`
    /// hears the picture at that size is shown.
    pub(crate) fn shown_on_main_thread(&self, answered: Option<PhysicalSize>) {
        self.showing().shown_on_main_thread();
        if let Some(size) = answered {
            self.tell_shown(size);
        }
    }

    /// Shows on this thread the picture `shown` carries, which could not reach the main thread,
    /// so its buffer goes back.
    pub(crate) fn show_here(&self, shown: ToShow) {
        match shown {
            ToShow::Picture(picture) | ToShow::Resize(_, Some(picture)) => self.show(picture),
            ToShow::Resize(_, None) => {}
        }
    }

    /// Sends `shown` to the main thread, in its turn.
    fn show_on_main_thread(&self, shown: ToShow) {
        match self.me.upgrade() {
            Some(me) => self.canvases.show(self.key, shown, me),
            None => self.show_here(shown),
        }
    }

    /// Tells the window its view shows the program's picture at `size`.
    fn tell_shown(&self, size: PhysicalSize) {
        (self.send)(ToWindow::PictureShown {
            window: self.window,
            size,
        });
    }

    fn showing(&self) -> MutexGuard<'_, Showing> {
        self.showing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Canvas for CanvasPictures {
    /// Shows the picture at once; or on the main thread, after what went there before it, laid
    /// out with the resize the window waits in when it is of that resize's size.
    fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        let picture = Picture::new(buffer, layers, released);
        let size = picture.size();
        let route = self.showing().presented(size);
        match route {
            Route::Canvas { first } => {
                self.show(picture);
                if first {
                    self.tell_shown(size);
                }
            }
            Route::MainThread => self.show_on_main_thread(ToShow::Picture(picture)),
            Route::Answer(resize) => {
                self.show_on_main_thread(ToShow::Resize(resize, Some(picture)));
            }
        }
    }
}

impl ToShow {
    /// The size of the resize it answers; none for a picture alone.
    pub(crate) fn answered(&self) -> Option<PhysicalSize> {
        match self {
            ToShow::Picture(_) => None,
            ToShow::Resize(resize, _) => Some(size_of(&resize.metrics)),
        }
    }
}

impl Showing {
    /// A picture of `size` was presented: where it goes.
    fn presented(&mut self, size: PhysicalSize) -> Route {
        let first = self.last.replace(size).is_none();
        let answered = self
            .awaited
            .take_if(|resize| size_of(&resize.metrics) == size);
        if let Some(resize) = answered {
            self.on_main_thread += 1;
            return Route::Answer(resize);
        }
        if self.on_main_thread > 0 {
            self.on_main_thread += 1;
            return Route::MainThread;
        }
        Route::Canvas { first }
    }

    /// The window reports the view's new `metrics`, with the `fence` it waits for the program's
    /// picture at that size with: a newer report takes the place of a resize awaited.
    fn resized(&mut self, metrics: Metrics, fence: SendRight) -> Placing {
        self.awaited = None;
        let resize = AwaitedResize { metrics, fence };
        match self.last {
            None => Placing::Now(resize.fence),
            Some(last) if last == size_of(&metrics) => {
                self.on_main_thread += 1;
                Placing::Answered(resize)
            }
            Some(_) => {
                self.awaited = Some(resize);
                Placing::Awaited
            }
        }
    }

    fn shown_on_main_thread(&mut self) {
        self.on_main_thread = self.on_main_thread.saturating_sub(1);
    }
}

/// The physical size `metrics` give a view.
fn size_of(metrics: &Metrics) -> PhysicalSize {
    [metrics.width, metrics.height]
}

#[cfg(test)]
mod tests;
