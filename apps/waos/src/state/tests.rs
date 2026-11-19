//! The programs' windows under a desktop with no window server: Waft's test platform makes a
//! window for each request at once, on its one screen, a compositor of the tests' own counts the
//! hardware buffers each composition layer is shown, and a vsync waiter of the tests' own serves
//! every wait at the host's next turn. The test plays the host: it reports each window made and
//! each one closed, draws the frame each view asked for, serves the waits for the screen's vsync,
//! and runs the app's tasks, as Waft's host does on its loop.
//!
//! Survey asks for a view, sees it arrive with what it starts with, keeps its window between the
//! size limits it sets, hears a resize, closes its window when asked to and ends once the view is
//! gone, moves its window from the band it treats as its title bar, asked for with no title bar of
//! macOS's own, and hears its window maximized, and its state follows its window's focus
//! and whether the window is minimized or occluded; triangle draws on its surface, and its window
//! closes when it is stopped; composition-layers' layers are the children of its canvas's layer,
//! where each present places them; and the clipboard program reads, through Waft's clipboard, what
//! another app copied, puts its own copy there, whose promised forms and file a paste from Waft's
//! clipboard has it produce, hears another app's copy replace it, and ends as its window is asked
//! to close; and the menus program has no menu bar in WaOS's own windows, WaOS's own bar staying
//! while its window is in front, hears Edit's commands through its canvas, opens a context menu
//! through Waft's at a key, and adds a tray icon, which offers the menus Waft's icon offers,
//! hears what Waft reports of it, opens a menu at it, and goes with the program, or fails with
//! why Waft's tray refused it; and a key it leaves is offered to the menu bar, and one it handles
//! is not.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::executor::block_on;
use waft::ui::{
    Clipboard as WaftClipboard, ClipboardEvent, CompositionLayer, Compositor, Contact, CopyRequest,
    Dispatcher, EditCommand, EmbedderClient, FileDialogs as WaftFileDialogs, Frame, HardwareBuffer,
    KeyData, KeyEventType, MenuBar, MenuEntry, MenuShortcut, Offset, PickedFile, Placement,
    Platform, PlatformRef, PointerDeviceKind, PointerReport, PopupMenus, Rect, ReportedChange,
    RequestId, Screen, ScreenId, Screens, ShortcutModifiers, TargetPlatform, TrayIconEvent,
    TrayIcons, URI_LIST, View, ViewError, ViewFocusDirection, ViewFocusEvent, ViewFocusState,
    ViewId, ViewRef, VsyncObserver, VsyncWaiter, WindowConfig,
};
use waft::{Entity, HostWindow, MacosTitleBar, Shell, run_widget};
use waft_test::{
    AskedDialog, MockClipboard, MockFileDialogs, MockMenuBar, MockPopupMenus, MockTrayIcons,
    TestPlatform, TestWindows,
};
use waos::{
    EngineTime, FreezePolicy, HttpProxy, OutputStream, ProgramEnd, ProgramEnvironment,
    ProgramOutput, RunningProgram, WaOS, WaOSConfig,
};
use waos_package::ProgramId;
use waos_program_window::WaftCompositor;

use super::ViewWindows;
use super::clipboard::Clipboard;
use super::file_dialogs::FileDialogs;
use super::menus::Menus;
use crate::logic::machine_facts::platform_state;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::prompts::PromptsOnRequest;
use crate::logic::vsync_waiter::WaftVsync;
use crate::state::questions::{self, Questions};
use crate::ui::every_window::{every_window, program_windows};
use waos_store::{SourceProxy, Store};

/// How long a test waits for a program to do something.
const WITHIN: Duration = Duration::from_secs(60);

/// The desktop's one screen, which every window is made on.
const SCREEN: ScreenId = ScreenId(1);

/// A desktop of the tests' own, with WaOS's windows on it and an engine whose views they place,
/// and a clipboard in memory, which the engine's programs reach through the app.
struct Desktop {
    shell: Shell,
    platform: Rc<DesktopPlatform>,
    engine: Arc<WaOS>,
    clipboard: MockClipboard,
    store: Store,
    view_windows: Entity<ViewWindows>,
    /// How many of the windows made the app has heard of, and which closed ones.
    reported_made: usize,
    reported_closed: Vec<ViewId>,
    started: Instant,
    folder: std::path::PathBuf,
}

impl Desktop {
    fn new(test: &str) -> Desktop {
        Desktop::with_tray(test, |tray| tray)
    }

    /// A desktop whose tray is as `tray` makes Waft's mock one: its icons offering both menus, as
    /// macOS's do, unless it says otherwise.
    fn with_tray(test: &str, tray: impl FnOnce(MockTrayIcons) -> MockTrayIcons) -> Desktop {
        let folder =
            std::env::temp_dir().join(format!("waos-desktop-{}-{test}", std::process::id()));
        let store = Store::open(&folder.join("store"), SourceProxy::None).expect("a store");
        let (placement, requests) = WindowsOnRequest::new(ProgramFaces::new(store.clone(), None));
        let (prompts, prompted) = PromptsOnRequest::new(crate::logic::asking_now::AskingNow::new());
        let config = WaOSConfig {
            code_cache: waos_test_programs::CODE_CACHE.into(),
            freeze: FreezePolicy::default(),
            native_unwind_info: false,
            http_proxy: HttpProxy::None,
            time: EngineTime::Real,
            temporary_folders: folder.join("tmp"),
            audit: None,
            inspected_programs: None,
            hardware_buffers: waos::HardwareBufferMode::Auto,
        };
        let clipboard = MockClipboard::new().with_promised_files_folder(folder.join("pasted"));
        let platform = Rc::new(DesktopPlatform::new(&clipboard, tray));
        let facts = platform_state(platform.as_ref());
        let served = Clipboard::of(platform.as_ref()).expect("the desktop's clipboard");
        let menus = Menus::of(platform.as_ref());
        let file_dialogs = FileDialogs::of(platform.as_ref()).expect("the desktop's dialogs");
        let vsync = Arc::new(WaftVsync::new(
            Arc::clone(&platform.vsync) as Arc<dyn VsyncWaiter>
        ));
        let shell = waos::Shell {
            views: Arc::new(placement),
            vsync: Arc::clone(&vsync) as Arc<dyn waos::VsyncWaiter>,
            compositor: Some(Arc::new(WaftCompositor::new(
                Arc::clone(&platform.compositor) as Arc<dyn Compositor>,
            ))),
            prompts: Arc::new(prompts),
            clipboard: Some(served.shell()),
            // A program's menu bar is its app's, never WaOS's own windows'.
            menu_bar: None,
            context_menus: menus.context_menus(),
            tray_icons: menus.tray_icons(),
            audio_output_picker: None,
            recording_indicator: None,
            file_dialogs: Some(file_dialogs.shell()),
            opener: None,
            notifications: None,
            media_players: None,
            media_assets: None,
            image_codecs: None,
            video_frame_readers: None,
            cameras: None,
            simulated_cameras: None,
            machine_picker: None,
        };
        let engine = Arc::new(WaOS::new(config, shell, facts).expect("an engine"));
        vsync.attach(&engine);
        let view_windows = Rc::new(RefCell::new(None));
        let kept = Rc::clone(&view_windows);
        let (served_to, questions_store) = (Arc::clone(&engine), store.clone());
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            served.serve(app, served_to);
            let windows = ViewWindows::listen(app, requests);
            let prompt_windows = questions::prompt_windows(
                app,
                windows.clone(),
                ProgramFaces::new(questions_store.clone(), None),
            );
            Questions::listen(
                app,
                prompted.questions,
                prompt_windows.clone(),
                questions_store,
            );
            let program_windows = program_windows(&windows, &prompt_windows);
            run_widget(
                app,
                every_window(None, program_windows, prompt_windows, None),
            );
            menus.serve(app, windows.clone());
            file_dialogs.serve(app, windows.clone());
            *kept.borrow_mut() = Some(windows);
        });
        let view_windows = view_windows.borrow_mut().take().expect("the app started");
        Desktop {
            shell,
            platform,
            engine,
            clipboard,
            store,
            view_windows,
            reported_made: 0,
            reported_closed: Vec::new(),
            started: Instant::now(),
            folder,
        }
    }

    /// Runs the test program at `component`, its output into `output`.
    fn run(&self, component: &str, output: &Arc<Lines>) -> RunningProgram {
        let program = self
            .engine
            .load(std::path::Path::new(component))
            .expect("the program loads");
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(output) as Arc<dyn ProgramOutput>),
            ..ProgramEnvironment::default()
        };
        let (running, _given) = self
            .engine
            .run(&program, environment, Vec::new())
            .expect("the program runs");
        running
    }

    /// Runs the test program at `component` as the program `id`, with the store's settings for it,
    /// which start as the design's defaults, and its own folders, its output into `output`.
    fn run_as(&self, component: &str, id: &str, output: &Arc<Lines>) -> RunningProgram {
        let program = self
            .engine
            .load(std::path::Path::new(component))
            .expect("the program loads");
        let id: ProgramId = id.parse().expect("an id");
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(output) as Arc<dyn ProgramOutput>),
            grants: Some(self.store.grants(&id)),
            folders: Some(self.store.folders(&id).expect("its folders")),
            ..ProgramEnvironment::default()
        };
        let (running, _given) = self
            .engine
            .run(&program, environment, Vec::new())
            .expect("the program runs");
        running
    }

    /// Plays the host until `done`, failing after [`WITHIN`].
    fn pump_until(&mut self, what: &str, done: impl Fn(&Desktop) -> bool) {
        let deadline = Instant::now() + WITHIN;
        while !done(self) {
            assert!(Instant::now() < deadline, "{what} never happened");
            self.pump();
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// One turn of the host's loop: the windows made and closed are reported, the frame each view
    /// asked for is drawn, the vsyncs waited for come, and the app's tasks run.
    fn pump(&mut self) {
        let made = self.platform.windows.made();
        for window in &made[self.reported_made..] {
            self.shell.view_added(window.test_view().id());
        }
        self.reported_made = made.len();
        for window in &made {
            let view = window.test_view().id();
            if window.closes() > 0 && !self.reported_closed.contains(&view) {
                self.reported_closed.push(view);
                self.shell.view_removed(view);
            }
        }
        let elapsed = self.started.elapsed();
        let views = self.platform.inner.take_frame_requests();
        if !views.is_empty() {
            self.shell.frame(
                &views,
                Frame {
                    elapsed,
                    vsync: None,
                },
            );
        }
        self.platform.vsync.serve(Instant::now());
        self.shell.wake(elapsed);
    }

    fn windows_made(&self) -> usize {
        self.platform.windows.made().len()
    }

    /// The window of the first view made has the focus, as Waft reports it.
    fn focus_first_window(&mut self) {
        let view = self.platform.windows.made()[0].test_view().id();
        self.shell.view_focus_changed(ViewFocusEvent {
            view_id: view,
            state: ViewFocusState::Focused,
            direction: ViewFocusDirection::Undefined,
        });
    }

    /// Another app copies `text`, and Waft says the clipboard changed.
    fn copied_elsewhere(&mut self, text: &str) {
        self.clipboard.set_clipboard_data(Some(text.to_owned()));
        self.shell.clipboard_changed();
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// Waft's test platform, with windows made on request on its one screen, a compositor, the
/// screen's vsyncs, a clipboard, a menu bar, context menus that answer the item at [`CHOSEN`],
/// and a tray whose menus opened at an icon answer the same.
struct DesktopPlatform {
    inner: TestPlatform,
    windows: Rc<TestWindows>,
    compositor: Arc<TestCompositor>,
    vsync: Arc<TestVsync>,
    menu_bar: MockMenuBar,
    popup_menus: MockPopupMenus,
    tray: MockTrayIcons,
    /// Dialogs that answer each in turn with the paths `answer_dialogs` gave.
    file_dialogs: MockFileDialogs,
    dialog_answers: Rc<RefCell<std::collections::VecDeque<std::path::PathBuf>>>,
}

/// The item the user chooses in every menu opened at a point or at a tray icon.
const CHOSEN: u32 = 40;

/// The key the menu bar takes when it is offered: `B`'s.
const TAKEN: MenuShortcut = MenuShortcut {
    key: 'b' as u64,
    modifiers: ShortcutModifiers {
        meta: false,
        control: false,
        alt: false,
        shift: false,
    },
};

impl DesktopPlatform {
    fn new(
        clipboard: &MockClipboard,
        tray: impl FnOnce(MockTrayIcons) -> MockTrayIcons,
    ) -> DesktopPlatform {
        let windows = Rc::new(TestWindows::new().on_screen(SCREEN));
        let menu_bar = MockMenuBar::new().taking([TAKEN]);
        let popup_menus = MockPopupMenus::new().answering(Some(CHOSEN));
        let tray = tray(MockTrayIcons::new().answering(Some(CHOSEN)));
        let dialog_answers = Rc::new(RefCell::new(std::collections::VecDeque::new()));
        let answers = Rc::clone(&dialog_answers);
        let file_dialogs = MockFileDialogs::new().answering(move || {
            answers
                .borrow_mut()
                .pop_front()
                .map(PickedFile::Path)
                .into_iter()
                .collect()
        });
        DesktopPlatform {
            inner: TestPlatform::new()
                .on(TargetPlatform::MacOS)
                .with_windows(Rc::clone(&windows))
                .with_clipboard(Rc::new(clipboard.clone()))
                .with_menu_bar(Rc::new(menu_bar.clone()))
                .with_popup_menus(Rc::new(popup_menus.clone()))
                .with_tray_icons(Rc::new(tray.clone()))
                .with_file_dialogs(Rc::new(file_dialogs.clone())),
            windows,
            compositor: Arc::new(TestCompositor::default()),
            vsync: Arc::new(TestVsync::default()),
            menu_bar,
            popup_menus,
            tray,
            file_dialogs,
            dialog_answers,
        }
    }
}

impl Platform for DesktopPlatform {
    fn target_platform(&self) -> TargetPlatform {
        self.inner.target_platform()
    }

    fn request_frame(&self, view: ViewId) {
        self.inner.request_frame(view);
    }

    fn now(&self) -> waft::ui::Instant {
        self.inner.now()
    }

    fn dispatcher(&self) -> Arc<dyn Dispatcher> {
        self.inner.dispatcher()
    }

    fn views(&self) -> Vec<ViewRef> {
        self.inner.views()
    }

    fn view(&self, id: ViewId) -> Option<ViewRef> {
        self.inner.view(id)
    }

    fn request_view(
        &self,
        config: WindowConfig,
        initial_data: Option<String>,
    ) -> Result<RequestId, ViewError> {
        self.inner.request_view(config, initial_data)
    }

    fn compositor(&self) -> Option<Arc<dyn Compositor>> {
        Some(Arc::clone(&self.compositor) as Arc<dyn Compositor>)
    }

    fn vsync_waiter(&self) -> Option<Arc<dyn VsyncWaiter>> {
        Some(Arc::clone(&self.vsync) as Arc<dyn VsyncWaiter>)
    }

    fn screens(&self) -> Option<&dyn Screens> {
        Some(self)
    }

    fn clipboard(&self) -> Option<&dyn WaftClipboard> {
        self.inner.clipboard()
    }

    fn menu_bar(&self) -> Option<&dyn MenuBar> {
        self.inner.menu_bar()
    }

    fn popup_menus(&self) -> Option<&dyn PopupMenus> {
        self.inner.popup_menus()
    }

    fn tray_icons(&self) -> Option<&dyn TrayIcons> {
        self.inner.tray_icons()
    }

    fn file_dialogs(&self) -> Option<&dyn WaftFileDialogs> {
        self.inner.file_dialogs()
    }
}

/// The desktop's one screen, a laptop's at 1x.
impl Screens for DesktopPlatform {
    fn screens(&self) -> Vec<Screen> {
        let frame = Rect::from_ltwh(0.0, 0.0, 1440.0, 900.0);
        vec![Screen {
            id: SCREEN,
            name: "Test Display".to_owned(),
            frame,
            visible_frame: frame,
            scale: 1.0,
            refresh_rate: Some(60.0),
        }]
    }

    fn pointer_location(&self) -> Option<Offset> {
        None
    }
}

/// The screen's vsyncs: every wait is served at the host's next turn.
#[derive(Default)]
struct TestVsync {
    waiting: Mutex<Vec<Weak<dyn VsyncObserver>>>,
}

impl TestVsync {
    /// Every vsync waited for comes, at `at`, on this thread: the main one, its picture expected
    /// on screen at the same moment.
    fn serve(&self, at: Instant) {
        let waiting = std::mem::take(&mut *self.waiting.lock().unwrap());
        for observer in waiting.iter().filter_map(Weak::upgrade) {
            observer.vsync(at, at);
        }
    }
}

impl VsyncWaiter for TestVsync {
    fn next_vsync(&self, _screen: ScreenId, observer: Weak<dyn VsyncObserver>) {
        self.waiting.lock().unwrap().push(observer);
    }
}

/// Composition layers that show nothing: each counts the hardware buffers it is shown, keeps the
/// children each present showed, and lets go of each buffer at once.
#[derive(Default)]
struct TestCompositor {
    /// Each layer made, in order.
    layers: Arc<Mutex<Vec<Arc<TestLayer>>>>,
}

impl TestCompositor {
    /// How many hardware buffers the layers have been shown.
    fn presents(&self) -> usize {
        self.layers
            .lock()
            .unwrap()
            .iter()
            .map(|layer| layer.presents.load(Ordering::SeqCst))
            .sum()
    }

    /// Whether each layer made, in order, was made opaque.
    fn opaque(&self) -> Vec<bool> {
        let layers = self.layers.lock().unwrap();
        layers.iter().map(|layer| layer.opaque).collect()
    }

    /// The children each present to the layer made `index`th showed, each by the order it was
    /// made in, with its placement.
    fn children_presented(&self, index: usize) -> Vec<Vec<(usize, Placement)>> {
        let layers = self.layers.lock().unwrap();
        let presented = layers[index].children.lock().unwrap();
        presented
            .iter()
            .map(|children| {
                children
                    .iter()
                    .map(|(child, placement)| {
                        let made = layers
                            .iter()
                            .position(|layer| {
                                std::ptr::addr_eq(Arc::as_ptr(layer), Arc::as_ptr(child))
                            })
                            .expect("a child this compositor made");
                        (made, placement.clone())
                    })
                    .collect()
            })
            .collect()
    }
}

impl Compositor for TestCompositor {
    fn make_layer(&self, opaque: bool) -> Arc<dyn CompositionLayer> {
        let layer = Arc::new(TestLayer {
            opaque,
            presents: AtomicUsize::new(0),
            children: Mutex::default(),
        });
        self.layers.lock().unwrap().push(Arc::clone(&layer));
        layer
    }
}

/// The children one present showed, the bottom-most first.
type Children = Vec<(Arc<dyn CompositionLayer>, Placement)>;

struct TestLayer {
    opaque: bool,
    presents: AtomicUsize,
    /// The children each present showed, in order.
    children: Mutex<Vec<Children>>,
}

impl CompositionLayer for TestLayer {
    fn present(
        &self,
        buffer: Arc<dyn HardwareBuffer>,
        children: Children,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        self.children.lock().unwrap().push(children);
        self.presents.fetch_add(1, Ordering::SeqCst);
        drop(buffer);
        released();
    }

    /// Nothing: each buffer is let go of as it is presented.
    fn current_picture(&self) -> Option<waft::ui::LayerPicture> {
        None
    }
}

/// What a program wrote to its standard output, line by line.
#[derive(Default)]
struct Lines {
    bytes: Mutex<Vec<u8>>,
}

impl Lines {
    fn all(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.bytes.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn has(&self, line: &str) -> bool {
        self.all().iter().any(|written| written == line)
    }
}

impl ProgramOutput for Lines {
    fn poll_write(
        &self,
        stream: OutputStream,
        _cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<usize> {
        if matches!(stream, OutputStream::Stdout) {
            self.bytes.lock().unwrap().extend_from_slice(bytes);
        }
        std::task::Poll::Ready(bytes.len())
    }
}

fn has_ended(program: &RunningProgram) -> Option<ProgramEnd> {
    program.ended().now_or_never()
}

#[test]
fn a_requested_view_arrives_in_its_window_hears_a_resize_and_goes_with_its_window() {
    let mut desktop = Desktop::new("survey");
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    desktop.pump_until("survey's view arriving", |_| {
        output.has(
            "view 1: 900x600 at 1x, on screen Some(1), in a window: true, for request Some(1) \
             with Some(\"opened at start\")",
        )
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    assert_eq!(window.config().title, "Survey");
    desktop.pump_until("survey's size limits reaching its window", |_| {
        window.max_size().is_some()
    });
    assert_eq!(window.min_size(), Some(waft::Size::new(320.0, 240.0)));
    assert_eq!(window.max_size(), Some(waft::Size::new(1920.0, 1200.0)));

    let view = window.test_view().id();
    window
        .test_view()
        .set_physical_size(waft::ui::Size::new(400.0, 300.0));
    desktop.shell.view_metrics_changed(view);
    desktop.pump_until("survey hearing the resize", |_| {
        output.has("view 1: now 400x300")
    });

    desktop.shell.view_close_requested(view);
    desktop.pump_until("survey closing its window", |_| window.closes() == 1);
    desktop.pump_until("survey ending", |_| has_ended(&survey).is_some());
    let end = has_ended(&survey).unwrap();
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert!(output.has("view 1: asked to close"), "{:#?}", output.all());
    assert!(output.has("view 1 removed"), "{:#?}", output.all());
    assert_eq!(desktop.windows_made(), 1);
}

#[test]
fn a_launch_for_a_program_with_no_window_yet_gives_its_next_window_the_launchs_token() {
    let mut desktop = Desktop::new("launch-token");
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    let token = "gnome-shell/Survey/2313-3_TIME121955272";
    let windows = desktop.view_windows.clone();
    windows.update(&mut desktop.shell.app(), |windows, cx| {
        windows.bring_forward_for_launch(survey.name(), Some(token.to_owned()), cx);
    });
    desktop.pump_until("survey's window", |desktop| desktop.windows_made() == 1);
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    assert_eq!(
        window.config().linux.activation_token.as_deref(),
        Some(token)
    );
    assert_eq!(
        window.config().linux.app_id,
        None,
        "a program never installed has no identity"
    );
}

#[test]
fn a_views_presence_follows_its_windows_focus_and_whether_it_is_minimized_or_occluded() {
    let mut desktop = Desktop::new("presence");
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    desktop.pump_until("survey's view arriving", |_| {
        output
            .all()
            .iter()
            .any(|line| line.starts_with("view 1: 900x600"))
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    let view = window.test_view().id();
    let focus = |state| ViewFocusEvent {
        view_id: view,
        state,
        direction: ViewFocusDirection::Undefined,
    };
    let steps = [
        (
            Step::Focus(ViewFocusState::Unfocused),
            "program: LifecycleState::Inactive",
        ),
        (Step::Occluded(true), "program: LifecycleState::Hidden"),
        (Step::Occluded(false), "program: LifecycleState::Inactive"),
        (Step::Minimized(true), "program: LifecycleState::Hidden"),
        (Step::Minimized(false), "program: LifecycleState::Inactive"),
        (
            Step::Focus(ViewFocusState::Focused),
            "program: LifecycleState::Resumed",
        ),
    ];
    for (step, heard) in steps {
        let before = output.all().len();
        match step {
            Step::Focus(state) => desktop.shell.view_focus_changed(focus(state)),
            Step::Occluded(occluded) => {
                window.set_occluded(occluded);
                desktop.shell.window_changed(view);
            }
            Step::Minimized(minimized) => {
                window.set_minimized(minimized);
                desktop.shell.window_changed(view);
            }
        }
        desktop.pump_until(heard, |_| {
            output.all()[before..].iter().any(|line| line == heard)
        });
    }
    survey.stop();
}

#[test]
fn a_press_on_surveys_title_bar_moves_its_window_and_survey_hears_it_maximized() {
    let mut desktop = Desktop::new("title-bar");
    let output = Arc::new(Lines::default());
    let survey = desktop.run(waos_test_programs::SURVEY, &output);
    desktop.pump_until("survey's view arriving", |_| {
        output
            .all()
            .iter()
            .any(|line| line.starts_with("view 1: 900x600"))
    });
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    assert_eq!(
        window.config().macos.title_bar,
        MacosTitleBar::Hidden { height: 28.0 },
        "survey's window has no title bar of macOS's own, its buttons in a band 28 points tall"
    );
    let view = window.test_view().id();

    desktop.shell.pointer_reports(vec![PointerReport {
        view_id: view,
        time_stamp: Duration::ZERO,
        device: 1,
        kind: PointerDeviceKind::Mouse,
        position: [100.0, 10.0],
        change: ReportedChange::Down { buttons: 1 },
        contact: Contact::default(),
    }]);
    desktop.pump_until("survey moving its window", |_| window.drags() == 1);

    window.set_maximized(true);
    window.set_frame(Rect::from_ltwh(0.0, 0.0, 1200.0, 800.0), None);
    desktop.shell.window_changed(view);
    window
        .test_view()
        .set_physical_size(waft::ui::Size::new(1200.0, 800.0));
    desktop.shell.view_metrics_changed(view);
    desktop.pump_until("survey hearing its window maximized", |_| {
        output.has("view 1: its window is maximized")
    });
    survey.stop();
}

/// What the host reports in the presence test: the window's focus, and the window changing as it
/// is covered or uncovered, minimized or restored.
enum Step {
    Focus(ViewFocusState),
    Occluded(bool),
    Minimized(bool),
}

/// Drawing makes hardware buffers, which only Apple's systems have so far.
#[cfg(target_vendor = "apple")]
#[test]
fn a_program_draws_on_its_surface_and_its_window_closes_when_it_ends() {
    let mut desktop = Desktop::new("triangle");
    let output = Arc::new(Lines::default());
    let triangle = desktop.run(waos_test_programs::TRIANGLE, &output);
    desktop.pump_until("triangle presenting ten frames", |desktop| {
        desktop.platform.compositor.presents() >= 10
    });
    triangle.stop();
    let window = Rc::clone(&desktop.platform.windows.made()[0]);
    desktop.pump_until("triangle's window closing", |_| window.closes() == 1);
    assert!(matches!(has_ended(&triangle), Some(ProgramEnd::Stopped)));
}

/// Presenting makes hardware buffers, which only Apple's systems have so far.
#[cfg(target_vendor = "apple")]
#[test]
fn a_programs_layers_are_shown_beneath_its_canvas_where_each_present_places_them() {
    use waft::ui::{Matrix4, Mutator, RRect, Radius};

    // The layers by the order Waft's compositor made them in: the window's canvas first, as the
    // window arrives, then the program's video and badge, once it has its view.
    const CANVAS: usize = 0;
    const VIDEO: usize = 1;
    const BADGE: usize = 2;
    let mut desktop = Desktop::new("composition-layers");
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::COMPOSITION_LAYERS, &output);
    desktop.pump_until("its two frames presented", |desktop| {
        desktop.platform.compositor.presents() >= 2
    });
    let compositor = &desktop.platform.compositor;
    assert_eq!(
        compositor.opaque(),
        [true, true, false],
        "the canvas as opaque as its window's solid background, the video opaque, the badge not"
    );
    let presented = compositor.children_presented(CANVAS);
    let scale_two = Matrix4::scale(2.0, 2.0);
    assert_eq!(
        presented[0][0],
        (
            VIDEO,
            Placement {
                rect: Rect::from_ltwh(0.0, 0.0, 640.0, 360.0),
                mutators: vec![
                    Mutator::Transform(scale_two),
                    Mutator::ClipRect(Rect::from_ltwh(0.0, 0.0, 600.0, 300.0)),
                    Mutator::ClipRRect(RRect::from_rect_and_radius(
                        Rect::from_ltwh(10.0, 10.0, 300.0, 200.0),
                        Radius::circular(8.0),
                    )),
                    Mutator::Opacity(128),
                ],
            }
        )
    );
    assert_eq!(
        presented[0][1],
        (
            BADGE,
            Placement {
                rect: Rect::from_ltwh(16.0, 16.0, 96.0, 48.0),
                mutators: Vec::new(),
            }
        )
    );
    assert_eq!(presented[0].len(), 2);
    assert_eq!(
        presented[1],
        [(
            BADGE,
            Placement {
                rect: Rect::from_ltwh(32.0, 32.0, 96.0, 48.0),
                mutators: vec![Mutator::Transform(scale_two)],
            }
        )],
        "the second frame lets the video go"
    );
    program.stop();
}

#[test]
fn a_program_reads_another_apps_copy_and_its_own_copy_is_pasted_from_waft_until_replaced() {
    let mut desktop = Desktop::new("clipboard");
    std::fs::create_dir_all(desktop.folder.join("pasted")).expect("the pasted files' folder");
    let output = Arc::new(Lines::default());
    let program = desktop.run_as(waos_test_programs::CLIPBOARD, "dev.waos.clipboard", &output);
    desktop.pump_until("its window", |_| output.has("a window"));
    desktop.focus_first_window();
    desktop.copied_elsewhere("copied elsewhere");
    desktop.pump_until("the other app's copy read", |_| {
        output.has("read: copied elsewhere")
    });
    desktop.pump_until("its own copy", |_| output.has("written"));

    let clipboard = desktop.clipboard.clone();
    assert_eq!(clipboard.clipboard_data().as_deref(), Some("copied text"));
    let options = clipboard.options();
    assert!(options.sensitive && !options.this_device_only);
    assert_eq!(options.expires_after, Some(Duration::from_secs(60)));
    let items = block_on(clipboard.items());
    assert_eq!(items[0].types(), ["text/plain", "text/html", "text/rtf"]);
    let html = block_on(items[0].read("text/html")).expect("its HTML, produced");
    assert_eq!(html, b"<b>copied text</b>");
    assert_eq!(items[1].types(), [URI_LIST]);
    let files = block_on(items[1].read(URI_LIST)).expect("its files");
    let files = String::from_utf8(files).expect("a list of URLs");
    let picture = desktop.folder.join("pasted").join("picture.png");
    assert_eq!(
        std::fs::read(&picture).expect("the picture, written"),
        b"PNG"
    );
    let names: Vec<&str> = files
        .lines()
        .map(|url| url.rsplit('/').next().unwrap_or_default())
        .collect();
    assert_eq!(names, ["note.txt", "picture.png"]);

    desktop.copied_elsewhere("another copy");
    desktop.pump_until("its copy replaced", |_| output.has("replaced"));
    let window = desktop.platform.windows.made()[0].test_view().id();
    desktop.shell.view_close_requested(window);
    desktop.pump_until("its end", |_| has_ended(&program).is_some());
    assert!(output.has("flushed"));
    assert!(matches!(
        has_ended(&program),
        Some(ProgramEnd::Returned(Ok(())))
    ));
}

#[test]
fn waos_own_bar_stays_while_a_programs_window_is_in_front_and_edits_commands_reach_the_program() {
    let mut desktop = Desktop::new("menu-bar");
    let bar = desktop.platform.menu_bar.clone();
    assert_eq!(
        titles(&bar.menus()),
        ["WaOS", "Edit", "Window"],
        "WaOS's own"
    );
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its window", |_| output.has("menu bar: none"));
    desktop.focus_first_window();
    desktop.pump();

    let view = desktop.platform.windows.made()[0].test_view().id();
    let request = CopyRequest::new(|_items, _options| Box::pin(std::future::ready(Ok(()))));
    desktop
        .shell
        .clipboard_event(view, ClipboardEvent::Copy(request));
    desktop.pump_until("the user's copy", |_| output.has("copy: written"));
    desktop
        .shell
        .clipboard_event(view, ClipboardEvent::Paste(Vec::new()));
    desktop.pump_until("the user's paste", |_| output.has("paste: copied by menus"));
    desktop.shell.edit_command(view, EditCommand::SelectAll);
    desktop.pump_until("select all", |_| output.has("edit: EditCommand::SelectAll"));

    assert_eq!(
        (titles(&bar.menus()), bar.sets()),
        (
            vec!["WaOS".to_owned(), "Edit".to_owned(), "Window".to_owned()],
            1
        ),
        "set once, as WaOS starts: a program's bar is its app's"
    );
    program.stop();
}

#[test]
fn a_programs_context_menu_and_tray_icon_go_through_wafts() {
    let mut desktop = Desktop::new("menus-at");
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its tray icon", |_| output.has("tray icon: added"));
    desktop.focus_first_window();
    desktop.pump();

    desktop.shell.key_data(key_down('m'));
    desktop.pump_until("the context menu's choice", |_| {
        output.has("context menu chose: Some(40)")
    });
    let shown = desktop.platform.popup_menus.shown();
    let view = desktop.platform.windows.made()[0].test_view().id();
    assert_eq!(
        (shown[0].view, shown[0].at),
        (view, Offset::new(10.0, 20.0))
    );

    let tray = desktop.platform.tray.clone();
    desktop.pump_until("the icon's attached menu", |_| {
        tray.icons()
            .first()
            .is_some_and(|icon| !icon.attached_menu.is_empty())
    });
    let icon = tray.icons()[0].clone();
    assert_eq!(icon.tooltip.as_deref(), Some("Menus"));
    let at = Offset::new(1.0, 2.0);
    desktop
        .shell
        .tray_icon_event(icon.id, TrayIconEvent::Activated { position: at });
    desktop.pump_until("the click", |_| output.has("tray: activated at 1,2"));
    desktop
        .shell
        .tray_icon_event(icon.id, TrayIconEvent::MenuSelected(30));
    desktop.pump_until("the attached menu's choice", |_| {
        output.has("attached menu chose: 30")
    });
    desktop.shell.tray_icon_event(
        icon.id,
        TrayIconEvent::ContextMenuRequested { position: at },
    );
    desktop.pump_until("the menu at the icon", |_| {
        output.has("menu at icon chose: Some(40)")
    });

    program.stop();
    desktop.pump_until("the icon removed", |_| tray.icons().is_empty());
}

#[test]
fn a_key_the_program_leaves_is_offered_to_the_menu_bar_and_one_it_handles_is_not() {
    let mut desktop = Desktop::new("key-equivalents");
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its window", |_| output.has("menu bar: none"));
    desktop.focus_first_window();
    desktop.pump();

    for key in ['a', 'b'] {
        desktop.shell.key_data(key_down(key));
    }
    let bar = desktop.platform.menu_bar.clone();
    let offered = || -> Vec<u64> { bar.offered().iter().map(|key| key.logical).collect() };
    desktop.pump_until("B, left, offered", |_| offered().contains(&u64::from('b')));
    assert!(output.has("key a: handled") && output.has("key b: left"));
    assert_eq!(offered(), [u64::from('b')], "A, handled, is not offered");
    program.stop();
}

#[test]
fn a_programs_tray_icon_offers_the_menus_wafts_icon_offers() {
    let mut desktop = Desktop::with_tray("tray-menus", MockTrayIcons::without_attached_menus);
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its tray icon", |_| output.has("tray icon: added"));
    assert!(output.has("tray icon menus: attached no, at icon yes"));
    let tray = desktop.platform.tray.clone();
    desktop.pump();
    assert_eq!(tray.icons()[0].attached_menu, [], "no attached menu set");
    program.stop();
}

#[test]
fn a_tray_icon_wafts_tray_refuses_fails_with_why() {
    let mut desktop = Desktop::with_tray("tray-refused", |tray| tray.refusing("no tray here"));
    let output = Arc::new(Lines::default());
    let program = desktop.run(waos_test_programs::MENUS, &output);
    desktop.pump_until("its tray icon refused", |_| {
        output.has("tray icon: failed (no tray here)")
    });
    assert!(desktop.platform.tray.icons().is_empty());
    program.stop();
}

#[test]
fn a_programs_file_dialogs_go_through_wafts_and_their_picks_are_mounted() {
    let mut desktop = Desktop::new("file-dialogs");
    let picks = std::fs::canonicalize(&desktop.folder)
        .unwrap()
        .join("picks");
    std::fs::create_dir_all(picks.join("pictures")).unwrap();
    std::fs::write(picks.join("read-me.txt"), "a line to read").unwrap();
    std::fs::write(picks.join("change-me.txt"), "a line").unwrap();
    for pick in ["read-me.txt", "change-me.txt", "saved.txt", "pictures"] {
        let answers = &desktop.platform.dialog_answers;
        answers.borrow_mut().push_back(picks.join(pick));
    }
    let output = Arc::new(Lines::default());
    let id = "dev.waos.filedialogs";
    let program = desktop.run_as(waos_test_programs::FILE_DIALOGS, id, &output);
    desktop.pump_until("its end", |_| has_ended(&program).is_some());
    for line in [
        "read file written to: no",
        "changed: yes",
        "saved safely: yes",
    ] {
        assert!(output.has(line), "no {line:?} in {:#?}", output.all());
    }
    assert_eq!(
        std::fs::read_to_string(picks.join("saved.txt")).unwrap(),
        "second save"
    );
    let asked = desktop.platform.file_dialogs.asked();
    assert!(matches!(
        &asked[2],
        AskedDialog::SaveFile(None, options) if options.suggested_name.as_deref() == Some("saved.txt")
    ));
    let kept = desktop.store.grants(&id.parse().unwrap()).file_system();
    assert!(
        kept.mounts
            .iter()
            .any(|mount| mount.host == picks.join("saved.txt") && mount.writable),
        "a pick is kept among the program's mounts: {kept:?}"
    );
}

/// The title of each top-level menu of `menus`.
fn titles(menus: &[MenuEntry]) -> Vec<String> {
    menus
        .iter()
        .filter_map(|menu| match menu {
            MenuEntry::Submenu { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

/// The key that types `character` going down, as a US keyboard's.
fn key_down(character: char) -> KeyData {
    KeyData {
        event_type: KeyEventType::Down,
        physical: 0x0007_0004 + u64::from(character as u32 - 'a' as u32),
        logical: u64::from(character),
        character: Some(character.to_string()),
        ..KeyData::default()
    }
}
