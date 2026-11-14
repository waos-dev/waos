use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver};
use waft::ui::{CompositionLayer, Compositor, HardwareBuffer, LayerPicture, Placement};
use waos::bindings::wasdk::platform::views::{
    Insets, KeyDevice, KeyKind, PointerButtons, PointerChange, PointerDeviceKind, PointerEvent,
    PointerSignalKind,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size, TitleBarAction};
use waos::{
    EngineTime, NoPrompts, OutputStream, Presence, ProgramEnd, ProgramEnvironment, ProgramOutput,
    ShellContextMenus, ShellMenuBar,
};
use waos_program_window::Canvas;
use waos_test_programs::CODE_CACHE;
use waos_window_messages::os::macos::ReceiveRight;
use waos_window_messages::{ViewCall, WindowCall};

use super::super::canvases::{CanvasCall, CanvasPlace};
use super::*;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::placement::tests::store_in;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::test_engine;

/// How long a test waits for a program to do something.
pub(crate) const WITHIN: Duration = Duration::from_secs(60);

/// The context id the test's main thread gives every canvas.
const CONTEXT: u32 = 0x5eed;

/// A window process served for a program, and what it hands on, each in order.
pub(crate) struct Served {
    pub(crate) process: Arc<WindowProcess>,
    /// What the window process was sent.
    pub(crate) sent: UnboundedReceiver<ToWindow>,
    /// The calls on its canvases.
    pub(crate) canvas_calls: UnboundedReceiver<CanvasCall>,
    /// What its prompts' dialogs reported.
    pub(crate) dialog_reports: UnboundedReceiver<DialogReport>,
    /// What it said macOS said of the program's notifications.
    pub(crate) system_permissions: UnboundedReceiver<SystemPermission>,
}

/// The window process of `program`'s app, as WaOS serves it once it said hello.
pub(crate) fn window_process(program: RunningProgram) -> Served {
    let (sent, arriving) = mpsc::unbounded();
    let send: SendToWindow = Arc::new(move |message| {
        let _ = sent.unbounded_send(message);
    });
    let (canvases, canvas_calls) = CanvasesOnRequest::new();
    let (dialog_reports, reported) = mpsc::unbounded();
    let (system_permissions, said) = mpsc::unbounded();
    let process = WindowProcess::new(
        program,
        send,
        Arc::new(canvases),
        dialog_reports,
        system_permissions,
        tokio::runtime::Handle::current(),
    );
    Served {
        process,
        sent: arriving,
        canvas_calls,
        dialog_reports: reported,
        system_permissions: said,
    }
}

/// Survey, running, and the window it asks for as it starts.
async fn survey() -> (RunningProgram, ViewRequest) {
    let folder = tempfile::tempdir().unwrap();
    let (placement, mut requests) =
        WindowsOnRequest::new(ProgramFaces::new(store_in(folder.path()), None));
    let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
    let survey = engine
        .load(Path::new(waos_test_programs::SURVEY))
        .expect("survey loads");
    let (program, _given) = engine
        .run(&survey, ProgramEnvironment::default(), Vec::new())
        .expect("survey runs");
    let asked = tokio::time::timeout(WITHIN, requests.next())
        .await
        .expect("survey asks for a window in time")
        .expect("a request");
    (program, asked.request)
}

/// A layer of Waft's that shows nothing.
struct Unshown;

impl CompositionLayer for Unshown {
    fn present(
        &self,
        _buffer: Arc<dyn HardwareBuffer>,
        _children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
        _released: Box<dyn FnOnce() + Send + Sync>,
    ) {
    }

    fn current_picture(&self) -> Option<LayerPicture> {
        None
    }
}

/// Waft's compositor, as the test's main thread has it.
pub(crate) struct Compositing;

impl Compositor for Compositing {
    fn make_layer(&self, _opaque: bool) -> Arc<dyn CompositionLayer> {
        Arc::new(Unshown)
    }
}

/// Carries out the next call on the canvases, which must be the making of one, as the main thread
/// does: the canvas is made, in the context `CONTEXT`.
async fn make_canvas(calls: &mut UnboundedReceiver<CanvasCall>) {
    let call = tokio::time::timeout(WITHIN, calls.next())
        .await
        .expect("a canvas is asked for in time")
        .expect("a call");
    let CanvasCall::Make {
        key,
        process,
        request,
    } = call
    else {
        panic!("the canvas is made first");
    };
    let canvas = Arc::new(CanvasLayer::make(&Compositing, request.config.background));
    process.canvas_made(
        key,
        request,
        canvas,
        HostedLayers {
            context_id: CONTEXT,
        },
    );
}

/// The next message the window process is sent.
pub(crate) async fn next(arriving: &mut UnboundedReceiver<ToWindow>) -> ToWindow {
    tokio::time::timeout(WITHIN, arriving.next())
        .await
        .expect("the window process is sent something in time")
        .expect("a message")
}

/// A window as its app reports it opened: one without size limits, so the program sets none and
/// its calls are the test's own; not maximized, on a Mac whose double-click on a title bar
/// maximizes.
fn opened() -> OpenedWindow {
    let none = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    OpenedWindow {
        metrics: Metrics {
            width: 800,
            height: 600,
            device_pixel_ratio: 2.0,
            padding: none,
            view_padding: none,
            view_insets: none,
        },
        screen: Some(1),
        presence: Presence {
            on_screen: true,
            occluded: false,
            focused: true,
        },
        frame: Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 400.0,
                height: 300.0,
            },
        },
        size_limits: false,
        maximized: false,
        title_bar_double_click_action: Some(TitleBarAction::Maximize),
    }
}

/// Which of a window's size limits a call sets, and the size, as width and height.
fn size_limit_of(call: ToWindow) -> (&'static str, Option<(f64, f64)>) {
    let as_pair = |size: Option<Size>| size.map(|size| (size.width, size.height));
    match call {
        ToWindow::View {
            call: ViewCall::Window(WindowCall::SetMinSize(size)),
            ..
        } => ("min", as_pair(size)),
        ToWindow::View {
            call: ViewCall::Window(WindowCall::SetMaxSize(size)),
            ..
        } => ("max", as_pair(size)),
        other => panic!("a size limit, not {other:?}"),
    }
}

/// A window whose app says it has size limits takes those the program sets, which go back to the
/// app as calls on that window: survey's smallest size, then its largest.
#[tokio::test]
async fn a_window_with_size_limits_takes_those_the_program_sets() {
    let (program, request) = survey().await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program.clone());
    process.open_window(request);
    make_canvas(&mut canvas_calls).await;
    let ToWindow::OpenWindow { window, .. } = next(&mut sent).await else {
        panic!("the window is asked of the program's app");
    };
    let limited = OpenedWindow {
        size_limits: true,
        ..opened()
    };
    process.received(
        FromWindow::WindowOpened {
            window,
            opened: limited,
        },
        Vec::new(),
    );

    assert_eq!(
        size_limit_of(next(&mut sent).await),
        ("min", Some((320.0, 240.0)))
    );
    assert_eq!(
        size_limit_of(next(&mut sent).await),
        ("max", Some((1920.0, 1200.0)))
    );
    program.stop();
}

/// A mouse going down on the band survey treats as its title bar, at `time` on the program's
/// clock, as the window's app reports it.
fn title_bar_pressed(window: RequestId, time: u64) -> FromWindow {
    let event = PointerEvent {
        time,
        change: PointerChange::Down,
        kind: PointerDeviceKind::Mouse,
        signal: PointerSignalKind::None,
        device: 1,
        x: 100.0,
        y: 10.0,
        buttons: PointerButtons::PRIMARY,
        pressure: 1.0,
        pressure_min: 0.0,
        pressure_max: 1.0,
        distance: 0.0,
        distance_max: 0.0,
        size: 0.0,
        radius_major: 0.0,
        radius_minor: 0.0,
        radius_min: 0.0,
        radius_max: 0.0,
        orientation: 0.0,
        tilt: 0.0,
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
        pan_x: 0.0,
        pan_y: 0.0,
        scale: 1.0,
        rotation: 0.0,
    };
    FromWindow::View {
        window,
        report: ViewReport::Pointer {
            event,
            held_keys: Vec::new(),
        },
    }
}

/// The call on a window the next message makes.
async fn window_call(sent: &mut UnboundedReceiver<ToWindow>) -> WindowCall {
    match next(sent).await {
        ToWindow::View {
            call: ViewCall::Window(call),
            ..
        } => call,
        other => panic!("a call on the window, not {other:?}"),
    }
}

/// A press on survey's title bar moves its window in its app, and a double-click there does what
/// the app said the system's setting is, to the window as the app reported it, both going back to
/// the app as calls on that window.
#[tokio::test]
async fn a_programs_title_bar_moves_maximizes_and_minimizes_its_window_in_its_app() {
    let (program, request) = survey().await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program.clone());
    process.open_window(request);
    make_canvas(&mut canvas_calls).await;
    let ToWindow::OpenWindow { window, .. } = next(&mut sent).await else {
        panic!("the window is asked of the program's app");
    };
    process.received(
        FromWindow::WindowOpened {
            window,
            opened: opened(),
        },
        Vec::new(),
    );
    let press =
        |time_ms: u64| process.received(title_bar_pressed(window, time_ms * 1_000_000), Vec::new());

    press(0);
    assert!(matches!(window_call(&mut sent).await, WindowCall::Drag));
    press(100);
    assert!(
        matches!(window_call(&mut sent).await, WindowCall::SetMaximized(true)),
        "maximized, as the app said a double-click does, from not maximized, as it opened"
    );

    let report = |report| process.received(FromWindow::View { window, report }, Vec::new());
    report(ViewReport::WindowMaximizedChanged(true));
    report(ViewReport::TitleBarDoubleClickActionChanged(Some(
        TitleBarAction::Minimize,
    )));
    press(1000);
    assert!(matches!(window_call(&mut sent).await, WindowCall::Drag));
    press(1100);
    assert!(
        matches!(window_call(&mut sent).await, WindowCall::SetMinimized(true)),
        "minimized, as the app reported the system's setting changed"
    );
    report(ViewReport::TitleBarDoubleClickActionChanged(Some(
        TitleBarAction::Maximize,
    )));
    press(2000);
    assert!(matches!(window_call(&mut sent).await, WindowCall::Drag));
    press(2100);
    assert!(
        matches!(
            window_call(&mut sent).await,
            WindowCall::SetMaximized(false)
        ),
        "returned to its size from before, as the app reported it maximized"
    );
    program.stop();
}

#[tokio::test]
async fn a_programs_window_opens_in_its_app_and_its_view_lives_there_until_it_closes() {
    let (program, request) = survey().await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program.clone());

    process.open_window(request);
    make_canvas(&mut canvas_calls).await;
    let ToWindow::OpenWindow {
        window,
        config,
        layers,
    } = next(&mut sent).await
    else {
        panic!("the window is asked of the program's app");
    };
    assert_eq!(config.title, "Survey");
    assert_eq!(layers.context_id, CONTEXT, "with where its canvas is");

    process.received(
        FromWindow::WindowOpened {
            window,
            opened: opened(),
        },
        Vec::new(),
    );
    let placed = canvas_calls.next().await;
    assert!(
        matches!(&placed, Some(CanvasCall::Place { metrics, fence: None, .. }) if metrics.width == 800),
        "the canvas is placed at the view's size as it opens"
    );

    let fence = ReceiveRight::new().unwrap();
    let mut bigger = opened().metrics;
    bigger.width = 1000;
    process.received(
        FromWindow::View {
            window,
            report: ViewReport::MetricsChanged(bigger),
        },
        vec![fence.make_send().unwrap()],
    );
    let placed = canvas_calls.next().await;
    assert!(
        matches!(&placed, Some(CanvasCall::Place { metrics, fence: Some(_), .. }) if metrics.width == 1000),
        "a new size before the view's first picture places the canvas at once, under the window's fence"
    );
    process.received(
        FromWindow::Key {
            window,
            key: 7,
            event: KeyEvent {
                time: 0,
                kind: KeyKind::Down,
                device: KeyDevice::Keyboard,
                physical: 0x0007_0004,
                logical: 0x61,
                character: Some("a".to_owned()),
                synthesized: false,
            },
        },
        Vec::new(),
    );
    assert!(
        matches!(
            next(&mut sent).await,
            ToWindow::KeyReplied {
                key: 7,
                handled: false
            }
        ),
        "survey handles no key, and its reply goes back"
    );

    process.received(
        FromWindow::View {
            window,
            report: ViewReport::CloseRequested,
        },
        Vec::new(),
    );
    let closed = next(&mut sent).await;
    assert!(
        matches!(
            closed,
            ToWindow::View {
                call: ViewCall::Window(WindowCall::Close),
                ..
            }
        ),
        "survey closes its window when asked: {closed:?}"
    );
    process.received(FromWindow::WindowClosed { window }, Vec::new());
    assert!(
        matches!(canvas_calls.next().await, Some(CanvasCall::Remove(_))),
        "the canvas goes with its window"
    );
    let end = tokio::time::timeout(WITHIN, program.ended())
        .await
        .expect("survey ends once its last view is gone");
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

/// The window's new size, with the fence it waits with, as its app reports it.
fn resized_to(process: &WindowProcess, window: RequestId, width: u32) {
    let mut metrics = opened().metrics;
    metrics.width = width;
    let fence = ReceiveRight::new().unwrap().make_send().unwrap();
    process.received(
        FromWindow::View {
            window,
            report: ViewReport::MetricsChanged(metrics),
        },
        vec![fence],
    );
}

/// The program may wait for what it awaits of its app before it draws, and the app carries it out
/// on the main thread its resize would wait on: the app's wait ends as the request arrives, so
/// WaOS lets the fence go then, and keeps none while the request is unanswered.
#[tokio::test]
async fn no_resize_waits_while_the_program_awaits_its_app() {
    let (program, request) = survey().await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program);
    process.open_window(request);
    make_canvas(&mut canvas_calls).await;
    let ToWindow::OpenWindow { window, .. } = next(&mut sent).await else {
        panic!("the window is asked of the program's app");
    };
    process.received(
        FromWindow::WindowOpened {
            window,
            opened: opened(),
        },
        Vec::new(),
    );
    let _opened = canvas_calls.next().await;
    let pictures = process.pictures_of(window).expect("the window's canvas");
    pictures.present(
        crate::os::test_buffer(800, 600),
        Vec::new(),
        Box::new(|| {}),
    );
    assert!(matches!(
        next(&mut sent).await,
        ToWindow::PictureShown {
            size: [800, 600],
            ..
        }
    ));

    resized_to(&process, window, 1000);
    assert!(
        canvas_calls.try_recv().is_err(),
        "the fence is kept for the picture at the new size"
    );
    let view = process.view(window).expect("the view").id();
    let _menu = process.show_context_menu(view, (1.0, 1.0), &[]);
    assert!(
        matches!(
            canvas_calls.next().await,
            Some(CanvasCall::Place { metrics, fence: Some(_), .. }) if metrics.width == 1000
        ),
        "a request the program awaits lets the resize go, laid out at once"
    );
    let ToWindow::ShowContextMenu { menu, .. } = next(&mut sent).await else {
        panic!("the menu is asked of the app");
    };

    resized_to(&process, window, 1200);
    assert!(
        matches!(
            canvas_calls.next().await,
            Some(CanvasCall::Place { metrics, fence: Some(_), .. }) if metrics.width == 1200
        ),
        "while the menu is unanswered, a new size is laid out at once"
    );

    process.received(
        FromWindow::ContextMenuClosed { menu, chosen: None },
        Vec::new(),
    );
    resized_to(&process, window, 1400);
    assert!(
        canvas_calls.try_recv().is_err(),
        "once it is answered, the fence is kept for the picture again"
    );
}

/// A window of 800 by 600 points on a screen of 2 pixels to the point, as its app reports it: the
/// program's view is its 1600 by 1200 pixels at 2 to the point, and the canvas shows what the
/// program draws at that size over the whole window.
#[tokio::test]
async fn a_2x_windows_view_gets_its_physical_size_and_ratio() {
    let (program, request) = survey().await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program);
    process.open_window(request);
    make_canvas(&mut canvas_calls).await;
    let ToWindow::OpenWindow { window, .. } = next(&mut sent).await else {
        panic!("the window is asked of the program's app");
    };
    let mut retina = opened();
    retina.metrics.width = 1600;
    retina.metrics.height = 1200;
    retina.frame.size = Size {
        width: 800.0,
        height: 600.0,
    };
    process.received(
        FromWindow::WindowOpened {
            window,
            opened: retina,
        },
        Vec::new(),
    );

    let view = process.view(window).expect("the view is added");
    let metrics = view.metrics();
    assert_eq!((metrics.width, metrics.height), (1600, 1200));
    assert_eq!(metrics.device_pixel_ratio, 2.0);
    let Some(CanvasCall::Place { metrics, .. }) = canvas_calls.next().await else {
        panic!("the canvas is placed as the window opens");
    };
    let place = CanvasPlace::of(&metrics);
    assert_eq!(
        place.placement.bounds(),
        waft::ui::Rect::from_ltwh(0.0, 0.0, 1600.0, 1200.0)
    );
    assert_eq!(
        place.placement.scale(),
        metrics.device_pixel_ratio,
        "a buffer of the view's size shows whole"
    );
    assert_eq!((place.root_width, place.root_height), (800.0, 600.0));
}

async fn next_report(reports: &mut UnboundedReceiver<DialogReport>) -> DialogReport {
    tokio::time::timeout(WITHIN, reports.next())
        .await
        .expect("a dialog reports in time")
        .expect("a report")
}

#[tokio::test]
async fn a_prompts_dialog_reports_to_the_main_thread_until_its_app_goes() {
    let (program, _request) = survey().await;
    let Served {
        process,
        mut sent,
        mut dialog_reports,
        ..
    } = window_process(program);
    let layers = HostedLayers {
        context_id: CONTEXT,
    };
    process.show_prompt(1, [440.0, 320.0], layers);
    assert!(matches!(
        next(&mut sent).await,
        ToWindow::ShowPrompt {
            prompt: 1,
            size: [440.0, 320.0],
            ..
        }
    ));
    process.received(
        FromWindow::Prompt {
            prompt: 1,
            report: PromptReport::InSight(false),
        },
        Vec::new(),
    );
    let report = next_report(&mut dialog_reports).await;
    assert_eq!(report.prompt, 1);
    assert!(matches!(report.report, PromptReport::InSight(false)));

    process.show_prompt(2, [440.0, 320.0], layers);
    process.hide_prompt(1);
    let _shown = next(&mut sent).await;
    assert!(matches!(
        next(&mut sent).await,
        ToWindow::HidePrompt { prompt: 1 }
    ));
    process.gone();
    let closed = next_report(&mut dialog_reports).await;
    assert_eq!(
        closed.prompt, 2,
        "the dialog still shown closes with its app"
    );
    assert!(matches!(closed.report, PromptReport::Closed));
    process.show_prompt(3, [440.0, 320.0], layers);
    let closed = next_report(&mut dialog_reports).await;
    assert_eq!(
        closed.prompt, 3,
        "a prompt shown once the app is gone closes at once"
    );
    assert!(matches!(closed.report, PromptReport::Closed));
}

/// The menu bar and context menus of the program's app, as `AppReach` serves them, served by
/// the one window process the test serves once it is there.
#[derive(Clone, Default)]
struct AppMenus(Arc<OnceLock<Arc<WindowProcess>>>);

impl ShellMenuBar for AppMenus {
    fn set(&self, _program: &RunningProgram, _entries: &[MenuEntry]) {
        if let Some(process) = self.0.get() {
            process.show_menu_bar();
        }
    }
}

impl ShellContextMenus for AppMenus {
    fn show(
        &self,
        _program: &RunningProgram,
        view: ViewId,
        x: f64,
        y: f64,
        entries: &[MenuEntry],
    ) -> MenuChosenFuture {
        match self.0.get() {
            Some(process) => process.show_context_menu(view, (x, y), entries),
            None => Box::pin(ready(None)),
        }
    }
}

/// What a program prints.
#[derive(Default)]
pub(crate) struct Printed(Mutex<Vec<u8>>);

impl ProgramOutput for Printed {
    fn write(&self, stream: OutputStream, bytes: &[u8]) {
        if matches!(stream, OutputStream::Stdout) {
            self.0.lock().unwrap().extend_from_slice(bytes);
        }
    }
}

impl Printed {
    /// Waits until the program has printed `line`.
    pub(crate) async fn line(&self, line: &str) {
        let printed = || {
            let bytes = self.0.lock().unwrap();
            String::from_utf8_lossy(&bytes)
                .lines()
                .any(|printed| printed == line)
        };
        tokio::time::timeout(WITHIN, async {
            while !printed() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("the program never printed {line:?}"));
    }
}

/// Menus, running with its app's menus, what it prints into `printed`, and the window it asks for
/// as it starts.
async fn menus(app_menus: &AppMenus, printed: &Arc<Printed>) -> (RunningProgram, ViewRequest) {
    let folder = tempfile::tempdir().unwrap();
    let (placement, mut requests) =
        WindowsOnRequest::new(ProgramFaces::new(store_in(folder.path()), None));
    let mut shell = test_engine::shell(Arc::new(placement), Arc::new(NoPrompts));
    shell.menu_bar = Some(Arc::new(app_menus.clone()));
    shell.context_menus = Some(Arc::new(app_menus.clone()));
    let engine = test_engine::engine_over(shell, EngineTime::Real, Path::new(CODE_CACHE));
    let menus = engine
        .load(Path::new(waos_test_programs::MENUS))
        .expect("menus loads");
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(printed) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let (program, _given) = engine
        .run(&menus, environment, Vec::new())
        .expect("menus runs");
    let asked = tokio::time::timeout(WITHIN, requests.next())
        .await
        .expect("menus asks for a window in time")
        .expect("a request");
    (program, asked.request)
}

/// Opens the window `request` asks for in the app `process` serves, as the app reports it open;
/// answers the request it was made for.
async fn open_in_app(
    process: &Arc<WindowProcess>,
    request: ViewRequest,
    sent: &mut UnboundedReceiver<ToWindow>,
    canvas_calls: &mut UnboundedReceiver<CanvasCall>,
) -> RequestId {
    process.open_window(request);
    make_canvas(canvas_calls).await;
    let ToWindow::OpenWindow { window, .. } = next(sent).await else {
        panic!("the window is asked of the program's app");
    };
    process.received(
        FromWindow::WindowOpened {
            window,
            opened: opened(),
        },
        Vec::new(),
    );
    window
}

/// The next message the window process is sent that `wanted` picks, those before it passed over.
pub(crate) async fn next_wanted(
    arriving: &mut UnboundedReceiver<ToWindow>,
    wanted: impl Fn(&ToWindow) -> bool,
) -> ToWindow {
    loop {
        let message = next(arriving).await;
        if wanted(&message) {
            return message;
        }
    }
}

/// The key at `M` going down in `window`, as the app sends it under `key`: menus opens its context
/// menu at it.
fn m_goes_down(window: RequestId, key: KeyId) -> FromWindow {
    FromWindow::Key {
        window,
        key,
        event: KeyEvent {
            time: 0,
            kind: KeyKind::Down,
            device: KeyDevice::Keyboard,
            physical: 0x0007_0010,
            logical: u64::from('m'),
            character: Some("m".to_owned()),
            synthesized: false,
        },
    }
}

/// The context menu the app is asked to open next.
async fn context_menu_asked(sent: &mut UnboundedReceiver<ToWindow>) -> ToWindow {
    next_wanted(sent, |message| {
        matches!(message, ToWindow::ShowContextMenu { .. })
    })
    .await
}

#[tokio::test]
async fn a_programs_menu_bar_is_its_apps_and_a_choice_in_it_comes_back_by_its_id() {
    let (app_menus, printed) = (AppMenus::default(), Arc::new(Printed::default()));
    let (program, request) = menus(&app_menus, &printed).await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program.clone());
    let _ = app_menus.0.set(Arc::clone(&process));
    open_in_app(&process, request, &mut sent, &mut canvas_calls).await;

    let set = next_wanted(&mut sent, |message| {
        matches!(message, ToWindow::SetMenuBar(_))
    })
    .await;
    let ToWindow::SetMenuBar(bar) = set else {
        unreachable!("a menu bar");
    };
    let greet = bar.iter().find(|entry| entry.id == 11).expect("Greet");
    assert_eq!(
        greet.parent,
        Some(10),
        "under File, whole as the program set it"
    );
    printed.line("menu bar: set").await;
    process.received(FromWindow::MenuBarChosen(11), Vec::new());
    printed.line("menu bar chose: 11").await;

    let Served {
        process: again,
        sent: mut sent_again,
        ..
    } = window_process(program);
    again.show_menu_bar();
    assert!(
        matches!(next(&mut sent_again).await, ToWindow::SetMenuBar(shown) if shown.len() == bar.len()),
        "an app connecting later is shown the bar the program set"
    );
}

#[tokio::test]
async fn a_programs_context_menu_opens_in_its_app_and_answers_once_closed_or_once_the_app_goes() {
    let (app_menus, printed) = (AppMenus::default(), Arc::new(Printed::default()));
    let (program, request) = menus(&app_menus, &printed).await;
    let Served {
        process,
        mut sent,
        mut canvas_calls,
        ..
    } = window_process(program);
    let _ = app_menus.0.set(Arc::clone(&process));
    let window = open_in_app(&process, request, &mut sent, &mut canvas_calls).await;
    printed.line("menu bar: set").await;

    process.received(m_goes_down(window, 1), Vec::new());
    let ToWindow::ShowContextMenu {
        menu,
        window: shown_in,
        at,
        entries,
    } = context_menu_asked(&mut sent).await
    else {
        unreachable!("a context menu");
    };
    assert_eq!(
        (shown_in, at),
        (window, (10.0, 20.0)),
        "in its view's pixels"
    );
    assert!(entries.iter().any(|entry| entry.id == 40), "Inspect");
    process.received(
        FromWindow::ContextMenuClosed {
            menu,
            chosen: Some(40),
        },
        Vec::new(),
    );
    printed.line("context menu chose: Some(40)").await;

    let unshown = process.view(window).expect("its view").id() + 1;
    assert_eq!(
        process
            .show_context_menu(unshown, (0.0, 0.0), &entries)
            .await,
        None,
        "a view the app does not show opens none"
    );

    process.received(m_goes_down(window, 2), Vec::new());
    context_menu_asked(&mut sent).await;
    process.gone();
    printed.line("context menu chose: None").await;
}

#[tokio::test]
async fn quitting_the_programs_app_asks_the_program_and_its_answer_reaches_the_app() {
    let (app_menus, printed) = (AppMenus::default(), Arc::new(Printed::default()));
    let (program, _request) = menus(&app_menus, &printed).await;
    let Served {
        process, mut sent, ..
    } = window_process(program.clone());

    process.received(FromWindow::QuitRequested, Vec::new());
    let answered = next_wanted(&mut sent, |message| {
        matches!(message, ToWindow::QuitAnswered(_))
    })
    .await;
    assert!(
        matches!(answered, ToWindow::QuitAnswered(AppExitResponse::Exit)),
        "a program that hears no exit request agrees at once: {answered:?}"
    );
    tokio::time::timeout(WITHIN, program.ended())
        .await
        .expect("a program that agreed to end is stopped");
}
