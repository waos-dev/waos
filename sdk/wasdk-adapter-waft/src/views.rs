//! What the framework draws into: one of the program's views as Waft's [`View`].
//!
//! The view's surface is made on the renderer's device when the view arrives; on
//! [`present`](View::present) the scene's slices are drawn as one picture, room made at each
//! composition layer, into the frame the surface hands out, the view's layers are set to those the
//! scene places beneath it (`composition`), and the queue presents the frame, unless the frame
//! changes nothing the view shows (`frame_drawing`). A video the scene draws shows its reader's frame for the
//! display time of the refresh being drawn (`shown_videos`), and at each refresh after, the view
//! takes the reader's new frame and draws its last scene again with it, without the framework, as
//! Waft's own hosts do. Where the view has a text input, the framework gets that too; where it is
//! in a window, the window.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use waft_embedder::{
    Picture, RequestId, Scene, ScreenId, Slices, SystemBars, SystemMouseCursorKind, TextInputHost,
    View, ViewConstraints, ViewFocusDirection, ViewFocusState, ViewHost, ViewId, ViewMetrics,
    ViewPadding, WindowRef, slices,
};
use wasdk::monotonic_clock;
use wasdk::views::{self, Insets, Instant, Metrics};

use crate::composition::placed_layers;
use crate::focus::{platform_focus_direction, platform_focus_state};
use crate::frame_drawing::ShownFrame;
use crate::mouse_cursor::cursor_of;
use crate::renderer::Renderer;
use crate::shown_videos::ShownVideos;
use crate::system_bars::WasdkSystemBars;
use crate::text_input::WasdkTextInput;
use crate::windows::WasdkWindow;

/// One view of the program, as the framework holds it.
pub(crate) struct WasdkView {
    id: ViewId,
    view: Arc<views::View>,
    /// What the view is drawn on, which holds a handle of its own to the view.
    surface: wgpu::Surface<'static>,
    renderer: Rc<Renderer>,
    /// Shared with the text input, which scales its rectangles by the device pixel ratio.
    metrics: Rc<Cell<ViewMetrics>>,
    text_input: Option<Rc<WasdkTextInput>>,
    /// The system's bars over the view; none where the platform draws none over it.
    system_bars: Option<WasdkSystemBars>,
    /// What a frame starts from: white, or nothing for a window that shows what is behind it.
    clear: valo::Color,
    /// What the frame on screen shows, which the next frame is drawn only to change.
    shown: RefCell<ShownFrame>,
    /// The request the view arrived for; none for a view the shell gave.
    request_id: Option<RequestId>,
    /// What the request attached.
    initial_data: Option<String>,
    /// The window the view is in, where it is in one; held weakly, since the window holds the
    /// view.
    window: RefCell<Weak<WasdkWindow>>,
    /// The videos its scenes draw, with the frame each shows.
    videos: ShownVideos,
    /// When the picture drawn now reaches the screen, as the refresh being drawn said, on the
    /// program's clock; none before the first.
    display_time: Cell<Option<Instant>>,
    /// The scene presented last, which a video's new frame draws again.
    last_scene: RefCell<Option<Arc<Scene>>>,
    /// Whether the framework asked for the view's next frame, which a video may ask for too.
    framework_asked: Cell<bool>,
}

impl WasdkView {
    /// The framework's view over `view`, drawn on a surface on `renderer`'s device, its frames
    /// starting from `clear`, its text input's host sending Edit's commands as
    /// `host_edit_commands` says.
    pub(crate) fn new(
        view: views::View,
        renderer: Rc<Renderer>,
        clear: valo::Color,
        host_edit_commands: bool,
    ) -> WasdkView {
        let view = Arc::new(view);
        let surface = renderer.surface(&view);
        let metrics = Rc::new(Cell::new(view_metrics(view.metrics(), screen_of(&view))));
        let text_input = view.text_input().map(|input| {
            let text_input = WasdkTextInput::new(input, Rc::clone(&metrics), host_edit_commands);
            Rc::new(text_input)
        });
        let system_bars = view.system_bars().map(WasdkSystemBars);
        WasdkView {
            id: ViewId(view.id()),
            request_id: Some(RequestId(view.request_id())),
            initial_data: view.initial_data(),
            view,
            surface,
            renderer,
            metrics,
            text_input,
            system_bars,
            clear,
            shown: RefCell::default(),
            window: RefCell::new(Weak::new()),
            videos: ShownVideos::default(),
            display_time: Cell::new(None),
            last_scene: RefCell::default(),
            framework_asked: Cell::new(false),
        }
    }

    /// The platform's view behind this one, which the platform asks for frames, cursors and
    /// focus.
    pub(crate) fn platform_view(&self) -> &views::View {
        &self.view
    }

    /// The view's text input, for the loop to type keys into and to tell what the platform did;
    /// the framework reaches it as [`View::text_input`].
    pub(crate) fn wasdk_text_input(&self) -> Option<Rc<WasdkTextInput>> {
        self.text_input.clone()
    }

    /// The view is in `window` from now on.
    pub(crate) fn set_window(&self, window: &Rc<WasdkWindow>) {
        *self.window.borrow_mut() = Rc::downgrade(window);
    }

    /// The view's geometry changed to `metrics`, and it may be on another screen.
    pub(crate) fn set_metrics(&self, metrics: Metrics) {
        self.metrics
            .set(view_metrics(metrics, screen_of(&self.view)));
    }

    /// Asks for the view's next frame for the framework.
    pub(crate) fn request_framework_frame(&self) {
        self.framework_asked.set(true);
        self.view.request_frame();
    }

    /// Whether the framework asked for the frame that came, which it draws; asked again before
    /// the next.
    pub(crate) fn take_framework_frame(&self) -> bool {
        self.framework_asked.replace(false)
    }

    /// The picture the view draws now reaches the screen at `display_time`, on the program's clock.
    pub(crate) fn set_display_time(&self, display_time: Instant) {
        self.display_time.set(Some(display_time));
    }

    /// Takes the new frames of the videos the last scene draws, at a refresh, and draws the scene
    /// again with them when any came; then asks for the next refresh while the view shows a
    /// video.
    pub(crate) fn draw_new_video_frames(&self) {
        if !self.videos.shows_video() {
            return;
        }
        if self
            .videos
            .take_new_frames(&self.renderer, self.display_time())
        {
            self.draw_last_scene();
        }
        self.view.request_frame();
    }

    /// Draws the scene presented last again, with the frames its videos show now.
    fn draw_last_scene(&self) {
        let Some(scene) = self.last_scene.borrow().clone() else {
            return;
        };
        let slices = self.slices_of(&scene);
        let frame = ShownFrame::of(&slices);
        self.draw(&slices.picture_with_room(), &frame);
        *self.shown.borrow_mut() = frame;
    }

    /// `scene`'s slices, each video in it drawn with the frame it shows now: a reader drawn for
    /// the first time shows its frame for the display time, and a reader the scene no longer draws
    /// is let go.
    fn slices_of(&self, scene: &Scene) -> Slices {
        let display_time = self.display_time();
        self.videos.begin_slicing();
        let slices = slices(scene, |reader| {
            self.videos.image_of(&self.renderer, reader, display_time)
        });
        self.videos.end_slicing();
        slices
    }

    /// When the picture drawn now reaches the screen: as the refresh being drawn said, or now
    /// before the first.
    fn display_time(&self) -> Instant {
        self.display_time.get().unwrap_or_else(monotonic_clock::now)
    }

    /// Draws `picture` into the frame the surface hands out at the view's size now, and puts it
    /// on screen with the layers `shown` places beneath it.
    fn draw(&self, picture: &Picture, shown: &ShownFrame) {
        let metrics = self.view.metrics();
        let size = [metrics.width, metrics.height];
        let Some(drawn) = self.renderer.draw(picture, &self.surface, size, self.clear) else {
            return;
        };
        self.view.set_layers(&placed_layers(shown.layers()));
        self.renderer.present(drawn);
    }
}

impl View for WasdkView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn metrics(&self) -> ViewMetrics {
        self.metrics.get()
    }

    fn present(&self, scene: Arc<Scene>) {
        let slices = self.slices_of(&scene);
        let frame = ShownFrame::of(&slices);
        if frame.changes(&self.shown.borrow()) {
            self.draw(&slices.picture_with_room(), &frame);
        }
        *self.shown.borrow_mut() = frame;
        *self.last_scene.borrow_mut() = Some(scene);
    }

    /// The WIT's view is the system end of what the framework asks for it.
    fn host(&self) -> &dyn ViewHost {
        self
    }

    fn initial_data(&self) -> Option<String> {
        self.initial_data.clone()
    }

    fn request_id(&self) -> Option<RequestId> {
        self.request_id
    }

    fn window(&self) -> Option<WindowRef> {
        let window = self.window.borrow().upgrade()?;
        Some(window as WindowRef)
    }
}

/// What the framework asks of the system for this view, carried by the WIT's view.
impl ViewHost for WasdkView {
    fn text_input(&self) -> Option<&dyn TextInputHost> {
        self.text_input
            .as_deref()
            .map(|text_input| text_input as &dyn TextInputHost)
    }

    fn activate_system_cursor(&self, _device: i64, kind: SystemMouseCursorKind) {
        self.view.set_cursor(cursor_of(kind));
    }

    fn request_focus_change(&self, state: ViewFocusState, direction: ViewFocusDirection) {
        self.view.request_focus_change(
            platform_focus_state(state),
            platform_focus_direction(direction),
        );
    }

    /// `set-handles-back`: whether the framework goes back within this view when the user asks.
    fn set_framework_handles_back(&self, handles: bool) {
        self.view.set_handles_back(handles);
    }

    /// `system-bars`: the bars over this view, which the framework styles as its annotated
    /// regions say after each frame, and hides and shows as its `SystemChrome` is told.
    fn system_bars(&self) -> Option<&dyn SystemBars> {
        self.system_bars
            .as_ref()
            .map(|bars| bars as &dyn SystemBars)
    }
}

/// The screen `view` is on now; none while it is on none.
fn screen_of(view: &views::View) -> Option<ScreenId> {
    view.screen().map(|screen| ScreenId(screen.id()))
}

/// A view's metrics in Waft's shape, on `screen`.
pub(crate) fn view_metrics(metrics: Metrics, screen: Option<ScreenId>) -> ViewMetrics {
    let width = f64::from(metrics.width);
    let height = f64::from(metrics.height);
    ViewMetrics {
        physical_size: [width, height],
        physical_constraints: ViewConstraints::tight(width, height),
        device_pixel_ratio: metrics.device_pixel_ratio,
        padding: view_padding(metrics.padding),
        view_padding: view_padding(metrics.view_padding),
        view_insets: view_padding(metrics.view_insets),
        // The interface's metrics carry no system gesture insets.
        system_gesture_insets: ViewPadding::ZERO,
        screen,
    }
}

fn view_padding(insets: Insets) -> ViewPadding {
    ViewPadding {
        left: insets.left,
        top: insets.top,
        right: insets.right,
        bottom: insets.bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Insets = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    #[test]
    fn metrics_are_carried_over_edge_for_edge() {
        let metrics = view_metrics(
            Metrics {
                width: 800,
                height: 600,
                device_pixel_ratio: 2.0,
                padding: Insets {
                    left: 1.0,
                    top: 2.0,
                    right: 3.0,
                    bottom: 4.0,
                },
                view_padding: NONE,
                view_insets: NONE,
            },
            Some(ScreenId(7)),
        );
        assert_eq!(metrics.physical_size, [800.0, 600.0]);
        assert_eq!(
            metrics.physical_constraints,
            ViewConstraints::tight(800.0, 600.0)
        );
        assert_eq!(metrics.device_pixel_ratio, 2.0);
        assert_eq!(
            metrics.padding,
            ViewPadding {
                left: 1.0,
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
            }
        );
        assert_eq!(metrics.view_insets, ViewPadding::ZERO);
        assert_eq!(metrics.screen, Some(ScreenId(7)));
    }
}
