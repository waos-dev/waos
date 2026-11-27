//! The program, as its window process reaches it: through WaOS, over the connection. Each call of
//! `waos-program-window`'s links is the message it is: a view added is `WindowOpened`, what a
//! window reports of its view a `ViewReport`, a key a `Key` whose reply comes back later. What a
//! view shows stays in WaOS: its canvas shows WaOS's layers for it, in a host of the layer context
//! WaOS named as it asked for the window (`hosted_layers`). A new size waits for the program's
//! picture at it: it goes with a fence of the window's transaction that commits it, and the window
//! waits inside the resize until WaOS says the program's picture at that size is shown
//! (`resize_waits`), WaOS having laid its layers out at that size with the picture under the fence,
//! so the two show together; WaOS answers at once where the picture it shows is of that size
//! already. Nothing waits before the view's first picture, nor while WaOS may serve the view no
//! frames, out of sight (off screen or occluded) or on no screen as the window last reported it,
//! since then no picture would come: the report that brings it back into sight could not leave a
//! main thread that waits; nor while the program awaits the app's answer to a request, which it may
//! wait for before it draws. An occluded window waits for nothing even while a tool watches the
//! program, when WaOS serves it frames.
//!
//! The invariant: a view's link names its window by the request it was made for, the name WaOS
//! knows it by, so a message never names a view WaOS has not heard of; and a fence goes to WaOS
//! only with a size the window waits at, since WaOS keeps it until the picture.

use std::cell::Cell;
use std::rc::Rc;

use waft::ui::PlatformRef;
use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{
    EditCommand, KeyEvent, Metrics, PointerEvent, RequestId, TextEditingValue, TextInputAction,
    ViewError, ViewFocusEvent,
};
use waos::bindings::wasdk::platform::windows::{Rect, TitleBarAction};
use waos::{HeldModifiers, KeyHandledFuture, Presence};
use waos_program_window::{CanvasLayer, OpenedView, ProgramLink, ViewLink};
use waos_window_messages::os::macos::SendRight;
use waos_window_messages::{FromWindow, OpenedWindow, ViewReport};

use crate::connection::Connection;
use crate::hosted_layers::show_hosted_layers;
use crate::os::LayerHost;

/// The program this window process is, reached through WaOS.
#[derive(Clone)]
pub(crate) struct WaosProgram {
    connection: Rc<Connection>,
    /// Waft's platform, whose macOS host shows WaOS's layers on a canvas.
    platform: PlatformRef,
}

/// One of the program's views, in the window made for its request `window`.
#[derive(Clone)]
pub(crate) struct WaosView {
    window: RequestId,
    connection: Rc<Connection>,
    /// The host of WaOS's layers its canvas shows; none where WaOS named none.
    host: Option<Rc<LayerHost>>,
    /// How the view stands, as the window last reported it.
    standing: Rc<ReportedStanding>,
}

/// How a view stands, as its window last reported it to WaOS, which serves it frames by it: while
/// it is in sight, on screen and not occluded, and on a screen. WaOS serves an occluded view frames
/// too while a tool watches its program, which the window is not told: an occluded window waits in
/// no resize, which nobody sees.
struct ReportedStanding {
    in_sight: Cell<bool>,
    screen: Cell<Option<ScreenId>>,
}

impl WaosProgram {
    pub(crate) fn new(connection: Rc<Connection>, platform: PlatformRef) -> WaosProgram {
        WaosProgram {
            connection,
            platform,
        }
    }

    /// Shows on `canvas` the layers WaOS keeps for the window made for `request`, in a host of
    /// their context; answers the host.
    fn show_layers(&self, request: RequestId, canvas: &CanvasLayer) -> Option<Rc<LayerHost>> {
        let Some(layers) = self.connection.take_layers(request) else {
            tracing::warn!(request, "WaOS named no layers for the window");
            return None;
        };
        let host = show_hosted_layers(&self.platform, &canvas.layer(), layers.context_id);
        Some(Rc::new(host))
    }
}

impl PartialEq for WaosProgram {
    fn eq(&self, other: &WaosProgram) -> bool {
        Rc::ptr_eq(&self.connection, &other.connection)
    }
}

impl ProgramLink for WaosProgram {
    type View = WaosView;

    fn name(&self) -> &str {
        self.connection.name()
    }

    fn ended(&self) -> impl Future<Output = ()> + 'static {
        self.connection.ended()
    }

    /// The view WaOS adds once it hears the window opened, its canvas showing WaOS's layers for
    /// it; the program's calls on it arrive from WaOS and go to the window's queue.
    fn add_view(&self, request: RequestId, opened: OpenedView) -> WaosView {
        let host = self.show_layers(request, &opened.canvas);
        self.connection.route(request, opened.calls);
        let window = OpenedWindow {
            metrics: opened.metrics,
            screen: opened.screen,
            presence: opened.presence,
            frame: opened.frame,
            size_limits: opened.size_limits,
            maximized: opened.maximized,
            fullscreen: opened.fullscreen,
            title_bar_double_click_action: opened.title_bar_double_click_action,
        };
        self.connection.send(FromWindow::WindowOpened {
            window: request,
            opened: window,
        });
        WaosView {
            window: request,
            connection: Rc::clone(&self.connection),
            host,
            standing: Rc::new(ReportedStanding {
                in_sight: Cell::new(opened.presence.in_sight()),
                screen: Cell::new(opened.screen),
            }),
        }
    }

    fn view_request_failed(&self, request: RequestId, error: ViewError) {
        self.connection.take_layers(request);
        self.connection.send(FromWindow::WindowRefused {
            window: request,
            error,
        });
    }

    fn remove_view(&self, view: &WaosView) {
        self.connection.unroute(view.window);
        self.connection.waits().forget(view.window);
        self.connection.send(FromWindow::WindowClosed {
            window: view.window,
        });
    }
}

impl WaosView {
    /// The window the view is in, by the request it was made for.
    pub(crate) fn window(&self) -> RequestId {
        self.window
    }

    fn report(&self, report: ViewReport) {
        self.connection.send(FromWindow::View {
            window: self.window,
            report,
        });
    }

    /// The fence of the window's transaction that commits a new size, for a resize that waits for
    /// the program's picture at it: once the view showed a picture, while WaOS serves it frames and
    /// the program awaits no answer of the app, and while it shows WaOS's layers, whose fence it
    /// is.
    fn fence_to_wait_with(&self) -> Option<SendRight> {
        let served = self.standing.in_sight.get() && self.standing.screen.get().is_some();
        if !served || !self.connection.waits().awaits(self.window) {
            return None;
        }
        self.host.as_ref()?.fence()
    }
}

impl ViewLink for WaosView {
    fn pointer(&self, event: PointerEvent, modifiers: &HeldModifiers) {
        let held_keys = modifiers.keys().collect();
        self.report(ViewReport::Pointer { event, held_keys });
    }

    fn key(&self, event: KeyEvent) -> KeyHandledFuture {
        self.connection.key(self.window, event)
    }

    fn focus(&self, event: ViewFocusEvent) {
        self.report(ViewReport::Focus(event));
    }

    fn text_input_editing_value(&self, value: TextEditingValue) {
        self.report(ViewReport::TextInputEditingValue(value));
    }

    fn text_input_action(&self, action: TextInputAction) {
        self.report(ViewReport::TextInputAction(action));
    }

    fn text_input_closed(&self) {
        self.report(ViewReport::TextInputClosed);
    }

    fn copy(&self) {
        self.report(ViewReport::Copy);
    }

    fn cut(&self) {
        self.report(ViewReport::Cut);
    }

    fn paste(&self) {
        self.report(ViewReport::Paste);
    }

    fn edit(&self, command: EditCommand) {
        self.report(ViewReport::Edit(command));
    }

    fn close_requested(&self) {
        self.report(ViewReport::CloseRequested);
    }

    fn pop_route(&self) {
        self.report(ViewReport::PopRoute);
    }

    /// Waits, inside the resize and with no timeout, until WaOS says the program's picture at the
    /// new size is shown, when a fence went with the size; or until WaOS asks the app for what the
    /// program awaits, or the connection ends, when the window shows its new size without that
    /// picture.
    fn metrics_changed(&self, metrics: Metrics) {
        let fence = self.fence_to_wait_with();
        let waits = fence.is_some();
        if waits {
            self.connection.waits().expect(self.window);
        }
        let report = FromWindow::View {
            window: self.window,
            report: ViewReport::MetricsChanged(metrics),
        };
        let sent = self
            .connection
            .send_with_ports(report, fence.into_iter().collect());
        if waits && sent {
            let size = [metrics.width, metrics.height];
            self.connection.waits().wait(self.window, size);
        }
    }

    fn screen_changed(&self, screen: Option<ScreenId>) {
        self.standing.screen.set(screen);
        self.report(ViewReport::ScreenChanged(screen));
    }

    fn presence_changed(&self, presence: Presence) {
        self.standing.in_sight.set(presence.in_sight());
        self.report(ViewReport::PresenceChanged(presence));
    }

    fn window_frame_changed(&self, frame: Rect) {
        self.report(ViewReport::WindowFrameChanged(frame));
    }

    fn window_maximized_changed(&self, maximized: bool) {
        self.report(ViewReport::WindowMaximizedChanged(maximized));
    }

    fn window_fullscreen_changed(&self, fullscreen: bool) {
        self.report(ViewReport::WindowFullscreenChanged(fullscreen));
    }

    fn title_bar_double_click_action_changed(&self, action: Option<TitleBarAction>) {
        self.report(ViewReport::TitleBarDoubleClickActionChanged(action));
    }
}
