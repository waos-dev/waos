//! `examples/two-windows`, a Waft app with two windows over `wasdk-adapter-waft`, under the test
//! shell, both on its one screen: both windows arrive titled `"first"` and `"second"` and draw
//! pictures; while the second window is hidden, reporting its view off screen, the first keeps
//! presenting frames and the second presents none; shown again, the second presents again; and
//! asking to close a window quits the program, its `main` returning.
//!
//! Hardware buffers are made on Apple's systems, where this test runs.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{
    Cursor, OverlayStyle, Placement, ViewError, ViewFocusDirection, ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{
    HardwareBuffer, OnRefresh, PlacedView, PlacedWindow, Presence, ProgramView, RunningProgram,
    ShellCompositionLayer, ShellView, ShellWindow, ShellWindowSizeLimits, ViewPlacement,
    ViewRequest, VsyncWaiter, WaOS, WeakProgramView,
};
use waos_headless::SCREEN;

use crate::test_shell::{
    DISPLAY, RecordedOutput, TestShell, WITHIN, config, metrics, printing_to, wait_until, within,
};

/// The titles two-windows asks its windows to have, in order.
const TITLES: [&str; 2] = ["first", "second"];

const OFF_SCREEN: Presence = Presence {
    on_screen: false,
    occluded: false,
    focused: false,
};
const ON_SCREEN: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: false,
};
const FOCUSED: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: true,
};

#[test]
fn a_hidden_window_gets_no_frames_while_the_shown_one_keeps_drawing_and_draws_again_once_shown() {
    let shell = TestShell::new();
    let vsync = Arc::new(ManualVsync::default());
    let (arrivals, arrived) = mpsc::channel();
    let placed = Arc::new(CountingPlacement {
        arrivals,
        views: Mutex::default(),
        titles: Mutex::default(),
    });
    let mut parts = shell.shell();
    parts.views = Arc::clone(&placed) as Arc<dyn ViewPlacement>;
    parts.vsync = Arc::clone(&vsync) as Arc<dyn VsyncWaiter>;
    let mut platform = shell.platform();
    // With no refresh rate reported, every frame comes strictly from a tick of `vsync`.
    platform.screens[0].refresh_rate = 0;

    let engine = WaOS::new(config(), parts, platform).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::TWO_WINDOWS))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let views: Vec<ProgramView> = TITLES
        .iter()
        .map(|_| {
            arrived
                .recv_timeout(WITHIN)
                .expect("two-windows asks for its windows")
        })
        .collect();
    assert_eq!(placed.titles(), TITLES);
    let counting = placed.views();
    let (shown, hidden) = (&counting[0], &counting[1]);

    // Drive refreshes until both windows have presented a non-blank picture.
    let deadline = Instant::now() + WITHIN;
    while shown.presents() == 0 || hidden.presents() == 0 {
        assert!(
            Instant::now() < deadline,
            "both windows never presented: shown={}, hidden={}",
            shown.presents(),
            hidden.presents()
        );
        vsync.refresh_next();
        within(running.idle());
    }
    let first_picture = views[0].picture().unwrap().expect("first window picture");
    let second_picture = views[1].picture().unwrap().expect("second window picture");
    assert!(is_not_blank(&first_picture), "first window drew its spinner");
    assert!(
        is_not_blank(&second_picture),
        "second window drew its spinner"
    );
    vsync.wait_for_ask();

    views[1].presence_changed(OFF_SCREEN);
    let hidden_presents = hidden.presents();
    let shown_presents = shown.presents();
    for step in 1..=3 {
        vsync.refresh_next();
        wait_until(&format!("shown window presents frame {step}"), || {
            shown.presents() >= shown_presents + step
        });
        within(running.idle());
        vsync.wait_for_ask();
    }
    assert_eq!(
        hidden.presents(),
        hidden_presents,
        "the hidden window presents nothing while the shown one keeps drawing"
    );

    views[1].presence_changed(ON_SCREEN);
    let shown_before = shown.presents();
    vsync.refresh_next();
    wait_until("shown again, the second window presents", || {
        hidden.presents() > hidden_presents && shown.presents() > shown_before
    });

    views[0].close_requested();
    let end = within(running.ended());
    assert!(
        matches!(end, waos::ProgramEnd::Returned(Ok(()))),
        "asking to close a window quits two-windows, not {end:?}; it printed {:#?}",
        output.lines()
    );
}

/// Whether `picture` holds more than one pixel colour.
fn is_not_blank(picture: &PixelBuffer) -> bool {
    let first = &picture.bytes[..4];
    (0..picture.height as usize).any(|y| {
        let row = &picture.bytes[y * picture.row_bytes as usize..];
        (0..picture.width as usize).any(|x| &row[x * 4..x * 4 + 4] != first)
    })
}

/// A screen's vsync that ticks only when a test calls [`ManualVsync::refresh_next`].
#[derive(Default)]
struct ManualVsync {
    pending: Mutex<Option<OnRefresh>>,
    asked: Condvar,
}

impl ManualVsync {
    fn wait_for_ask(&self) {
        let deadline = Instant::now() + WITHIN;
        let mut pending = self.pending.lock().unwrap();
        while pending.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "the engine never asked for a refresh");
            pending = self.asked.wait_timeout(pending, left).unwrap().0;
        }
    }

    fn refresh_next(&self) {
        let on_refresh = {
            let deadline = Instant::now() + WITHIN;
            let mut pending = self.pending.lock().unwrap();
            while pending.is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                assert!(!left.is_zero(), "the engine never asked for a refresh");
                pending = self.asked.wait_timeout(pending, left).unwrap().0;
            }
            pending.take().unwrap()
        };
        let now = Instant::now();
        on_refresh(now, now + Duration::from_millis(16));
    }
}

impl VsyncWaiter for ManualVsync {
    fn next_vsync(&self, _screen: ScreenId, on_refresh: OnRefresh) {
        *self.pending.lock().unwrap() = Some(on_refresh);
        self.asked.notify_all();
    }
}

/// A view that counts the frames presented to it and releases each replaced buffer.
#[derive(Default)]
struct CountingView {
    presents: AtomicUsize,
    shown: Mutex<Option<Box<dyn FnOnce() + Send + Sync>>>,
}

impl CountingView {
    fn presents(&self) -> usize {
        self.presents.load(Ordering::SeqCst)
    }
}

impl ShellView for CountingView {
    fn present(
        &self,
        _buffer: HardwareBuffer,
        _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        let before = self.shown.lock().unwrap().replace(released);
        self.presents.fetch_add(1, Ordering::SeqCst);
        if let Some(before) = before {
            before();
        }
    }

    fn set_cursor(&self, _cursor: Cursor) {}
    fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}
    fn set_handles_back(&self, _handles: bool) {}
    fn set_overlay_style(&self, _style: OverlayStyle) {}
}

/// A window that removes its view when closed.
#[derive(Default)]
struct ClosingWindow {
    view: OnceLock<WeakProgramView>,
}

impl ShellWindow for ClosingWindow {
    fn set_frame(&self, _frame: Rect, _animate: Option<Duration>) {}
    fn set_title(&self, _title: &str) {}
    fn show(&self) {}
    fn hide(&self) {}
    fn close(&self) {
        if let Some(view) = self.view.get().and_then(WeakProgramView::upgrade) {
            view.remove();
        }
    }
    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
        None
    }
    fn drag(&self) {}
    fn set_maximized(&self, _maximized: bool) {}
    fn set_minimized(&self, _minimized: bool) {}
}

/// Places a [`CountingView`] per request and records the requested titles in order.
struct CountingPlacement {
    arrivals: mpsc::Sender<ProgramView>,
    views: Mutex<Vec<Arc<CountingView>>>,
    titles: Mutex<Vec<String>>,
}

impl CountingPlacement {
    fn views(&self) -> Vec<Arc<CountingView>> {
        self.views.lock().unwrap().clone()
    }

    fn titles(&self) -> Vec<String> {
        self.titles.lock().unwrap().clone()
    }
}

impl ViewPlacement for CountingPlacement {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        let shell_view = Arc::new(CountingView::default());
        let window = Arc::new(ClosingWindow::default());
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: f64::from(DISPLAY.width) / DISPLAY.scale,
                height: f64::from(DISPLAY.height) / DISPLAY.scale,
            },
        };
        self.titles.lock().unwrap().push(request.config.title);
        self.views.lock().unwrap().push(Arc::clone(&shell_view));
        let view = program.add_view(
            request.id,
            PlacedView {
                view: shell_view,
                text_input: None,
                window: Some(PlacedWindow {
                    window: Arc::clone(&window) as Arc<dyn ShellWindow>,
                    frame,
                    maximized: false,
                    title_bar_double_click_action: None,
                }),
                metrics: metrics(DISPLAY.width, DISPLAY.height),
                screen: Some(SCREEN),
                presence: FOCUSED,
            },
        );
        let _ = window.view.set(view.downgrade());
        let _ = self.arrivals.send(view);
        Ok(())
    }
}
