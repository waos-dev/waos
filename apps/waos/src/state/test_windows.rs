//! A desktop of the tests' own for the apps' windows, with no window server: Waft's test platform
//! makes a window for each request at once, on its one screen, with a clipboard in memory, a menu
//! bar, context menus and a tray of Waft's mocks, and file dialogs that answer what the test gives;
//! a compositor of the tests' own counts the hardware buffers each composition layer is shown, and
//! a vsync waiter of the tests' own serves every wait at the host's next turn. Over it, WaOS's one
//! startup over stores in memory, which the test fills with the apps its build made, its apps
//! opened through the app instances, their windows and the questions they put to their user shown
//! as the app shows them. The test plays the host: it reports each window made and each one
//! closed, draws the frame each view asked for, serves the waits for the screen's vsync, and runs
//! the app's tasks, as Waft's host does on its loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use tempfile::TempDir;
use waft::ui::{
    Clipboard as WaftClipboard, CompositionLayer, Compositor, Dispatcher, EmbedderClient,
    FileDialogs as WaftFileDialogs, Frame, HardwareBuffer, MenuBar, MenuShortcut, Offset,
    PickedFile, Placement, Platform, PlatformRef, PopupMenus, Rect, RequestId, Screen, ScreenId,
    Screens, ShortcutModifiers, TargetPlatform, TrayIcons, View, ViewError, ViewFocusDirection,
    ViewFocusEvent, ViewFocusState, ViewId, ViewRef, VsyncObserver, VsyncWaiter, WindowConfig,
};
use waft::{Entity, Shell, run_widget};
use waft_test::{
    MockClipboard, MockFileDialogs, MockMenuBar, MockPopupMenus, MockTrayIcons, TestPlatform,
    TestWindows,
};
use waos::AppInstance;
use waos_program_window::WaftCompositor;
use waos_services::packages::{InstalledAppsInMemory, InstalledAppsStore};
use waos_services::permissions::Prompts;

use super::ViewWindows;
use super::clipboard::Clipboard;
use super::file_dialogs::FileDialogs;
use super::library::work;
use super::menus::Menus;
use crate::logic::app_instances::AppInstances;
use crate::logic::machine_facts::platform_state;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::prompts::PromptsOnRequest;
use crate::logic::startup::{self, Outside, Setup};
use crate::logic::stores::Stores;
use crate::logic::test_engine::{self, NoWindows};
use crate::logic::test_installs;
use crate::logic::vsync_waiter::WaftVsync;
use crate::state::questions::{self, Questions};
use crate::state::test_system::{Lines, run_heard};
use crate::state::test_turns::{Turn, turn_until};
use crate::ui::every_window::{every_window, program_windows};

/// The desktop's one screen, which every window is made on.
const SCREEN: ScreenId = ScreenId(1);

/// A desktop of the tests' own, with WaOS's windows on it and an engine whose views they place,
/// and a clipboard in memory, which the engine's programs reach through the app.
pub(crate) struct Desktop {
    pub(crate) shell: Shell,
    pub(crate) platform: Rc<DesktopPlatform>,
    pub(crate) clipboard: MockClipboard,
    /// The installed apps, which the test fills with the apps its build made.
    installed: Arc<InstalledAppsInMemory>,
    pub(crate) instances: Arc<AppInstances>,
    /// What WaOS asks its user through, for a test to ask as an app's permissions do.
    pub(crate) prompts: Arc<PromptsOnRequest>,
    pub(crate) permissions: waos_services::permissions::Permissions,
    pub(crate) view_windows: Entity<ViewWindows>,
    pub(crate) questions: Entity<Questions>,
    /// How many of the windows made the app has heard of, and which closed ones.
    reported_made: usize,
    reported_closed: Vec<ViewId>,
    started: Instant,
    /// Where the app instances' tasks run.
    work: Arc<tokio::runtime::Runtime>,
    pub(crate) folder: TempDir,
}

impl Desktop {
    pub(crate) fn new() -> Desktop {
        Desktop::with_tray(|tray| tray)
    }

    /// A desktop whose tray is as `tray` makes Waft's mock one: its icons offering both menus, as
    /// macOS's do, unless it says otherwise.
    pub(crate) fn with_tray(tray: impl FnOnce(MockTrayIcons) -> MockTrayIcons) -> Desktop {
        let folder = tempfile::tempdir().unwrap();
        let clipboard =
            MockClipboard::new().with_promised_files_folder(folder.path().join("pasted"));
        let platform = Rc::new(DesktopPlatform::new(&clipboard, tray));
        let facts = platform_state(platform.as_ref());
        let served = Clipboard::of(platform.as_ref()).expect("the desktop's clipboard");
        let menus = Menus::of(platform.as_ref());
        let file_dialogs = FileDialogs::of(platform.as_ref()).expect("the desktop's dialogs");
        let vsync = Arc::new(WaftVsync::new(
            Arc::clone(&platform.vsync) as Arc<dyn VsyncWaiter>
        ));
        let installed = Arc::new(InstalledAppsInMemory::default());
        let stores = Stores {
            installed_apps: Arc::clone(&installed) as Arc<dyn InstalledAppsStore>,
            ..Stores::in_memory(Vec::new())
        };
        let work = work().unwrap();
        let setup = Setup::for_tests(folder.path(), stores, work.handle().clone());
        let prompts = || {
            let (prompts, prompted) = PromptsOnRequest::new();
            let prompts = Arc::new(prompts);
            (
                Arc::clone(&prompts) as Arc<dyn Prompts>,
                (prompts, prompted),
            )
        };
        let (started, (prompts, prompted, requests)) =
            startup::start(setup, prompts, |before, (prompts, prompted)| {
                let faces = ProgramFaces::new(Arc::clone(before.packages), None);
                let (placement, requests) = WindowsOnRequest::new(faces);
                let shell = waos::Shell {
                    views: Arc::new(placement),
                    vsync: Arc::clone(&vsync) as Arc<dyn waos::VsyncWaiter>,
                    compositor: Some(Arc::new(WaftCompositor::new(
                        Arc::clone(&platform.compositor) as Arc<dyn Compositor>,
                    ))),
                    clipboard: Some(served.shell()),
                    // A program's menu bar is its app's, never WaOS's own windows'.
                    menu_bar: None,
                    context_menus: menus.context_menus(),
                    tray_icons: menus.tray_icons(),
                    file_dialogs: Some(file_dialogs.shell()),
                    ..test_engine::shell(Arc::new(NoWindows))
                };
                let capabilities = waos_local::LocalCapabilities::new();
                let kept = (prompts, prompted, requests);
                Ok(Outside::new(shell, facts, capabilities, kept))
            })
            .expect("WaOS starts");
        let startup::Started {
            instances,
            packages,
            permissions,
            engine,
            ..
        } = started;
        let kept = Rc::new(RefCell::new(None));
        let shown = Rc::clone(&kept);
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            served.serve(app, engine);
            let windows = ViewWindows::listen(app, requests);
            let prompt_windows = questions::prompt_windows(
                app,
                windows.clone(),
                ProgramFaces::new(Arc::clone(&packages), None),
            );
            let questions =
                Questions::listen(app, prompted.questions, prompt_windows.clone(), packages);
            let program_windows = program_windows(&windows, &prompt_windows);
            run_widget(
                app,
                every_window(None, program_windows, prompt_windows, None),
            );
            menus.serve(app, windows.clone());
            file_dialogs.serve(app, windows.clone(), None, None);
            *shown.borrow_mut() = Some((windows, questions));
        });
        let (view_windows, questions) = kept.borrow_mut().take().expect("the app started");
        Desktop {
            shell,
            platform,
            clipboard,
            installed,
            instances,
            prompts,
            permissions,
            view_windows,
            questions,
            reported_made: 0,
            reported_closed: Vec::new(),
            started: Instant::now(),
            work,
            folder,
        }
    }

    /// Runs the test program at `component` as the app `dev.example.` and its file's name, named
    /// that name, its output into `output`.
    pub(crate) fn run(&self, component: &str, output: &Arc<Lines>) -> AppInstance {
        let stem = std::path::Path::new(component)
            .file_stem()
            .expect("a component's file")
            .to_string_lossy()
            .replace(['-', '_'], "");
        let mut name = stem.clone();
        name[..1].make_ascii_uppercase();
        self.run_as(component, &format!("dev.example.{stem}"), &name, output)
    }

    /// Runs the test program at `component` as the app `id`, named `name`, installed from the
    /// test's build and opened as a developer opens it, with the store's settings for it, which
    /// start as the design's defaults, and its own folders, its output into `output`.
    pub(crate) fn run_as(
        &self,
        component: &str,
        id: &str,
        name: &str,
        output: &Arc<Lines>,
    ) -> AppInstance {
        self.installed
            .put(test_installs::built_app(component, id, name));
        let host = self.platform.inner.dispatcher();
        let app = id.parse().expect("an id");
        run_heard(&self.instances, self.work.handle(), &app, output, host)
    }

    /// Plays the host until `done`, failing after `WITHIN`: a turn follows at once while the app
    /// has a frame, a window, a vsync or a timer to turn for, and otherwise once a task wakes it.
    pub(crate) fn pump_until(&mut self, what: &str, done: impl Fn(&Desktop) -> bool) {
        let platform = Rc::clone(&self.platform);
        turn_until(&platform.inner, what, || {
            if done(self) {
                return Turn::Done;
            }
            self.pump();
            if done(self) {
                Turn::Done
            } else if self.has_turns_to_take() {
                Turn::FrameAsked
            } else {
                Turn::Idle
            }
        });
    }

    /// Whether the host's loop has something to turn for without a wake: a frame asked for, a
    /// window made or closed that the app has not heard of, a vsync waited for, or a timer.
    fn has_turns_to_take(&self) -> bool {
        let made = self.platform.windows.made();
        !self.platform.inner.frame_requests().is_empty()
            || made.len() > self.reported_made
            || made.iter().any(|window| {
                window.closes() > 0 && !self.reported_closed.contains(&window.test_view().id())
            })
            || self.platform.vsync.is_waited_for()
            || self
                .platform
                .inner
                .wakes()
                .last()
                .is_some_and(Option::is_some)
    }

    /// One turn of the host's loop: the windows made and closed are reported, the frame each view
    /// asked for is drawn, the vsyncs waited for come, and the app's tasks run.
    pub(crate) fn pump(&mut self) {
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

    pub(crate) fn windows_made(&self) -> usize {
        self.platform.windows.made().len()
    }

    /// The window of the first view made has the focus, as Waft reports it.
    pub(crate) fn focus_first_window(&mut self) {
        let view = self.platform.windows.made()[0].test_view().id();
        self.shell.view_focus_changed(ViewFocusEvent {
            view_id: view,
            state: ViewFocusState::Focused,
            direction: ViewFocusDirection::Undefined,
        });
    }

    /// Another app copies `text`, and Waft says the clipboard changed.
    pub(crate) fn copied_elsewhere(&mut self, text: &str) {
        self.clipboard.set_clipboard_data(Some(text.to_owned()));
        self.shell.clipboard_changed();
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        self.work.block_on(self.instances.stop_all());
    }
}

/// Waft's test platform, with windows made on request on its one screen, a compositor, the
/// screen's vsyncs, a clipboard, a menu bar, context menus that answer the item at [`CHOSEN`],
/// and a tray whose menus opened at an icon answer the same.
pub(crate) struct DesktopPlatform {
    pub(crate) inner: TestPlatform,
    pub(crate) windows: Rc<TestWindows>,
    pub(crate) compositor: Arc<TestCompositor>,
    pub(crate) vsync: Arc<TestVsync>,
    pub(crate) menu_bar: MockMenuBar,
    pub(crate) popup_menus: MockPopupMenus,
    pub(crate) tray: MockTrayIcons,
    /// Dialogs that answer each in turn with the paths `answer_dialogs` gave.
    pub(crate) file_dialogs: MockFileDialogs,
    pub(crate) dialog_answers: Rc<RefCell<std::collections::VecDeque<std::path::PathBuf>>>,
}

/// The item the user chooses in every menu opened at a point or at a tray icon.
pub(crate) const CHOSEN: u32 = 40;

/// The key the menu bar takes when it is offered: `B`'s.
pub(crate) const TAKEN: MenuShortcut = MenuShortcut {
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
        let inner = TestPlatform::new()
            .on(TargetPlatform::MacOS)
            .with_windows(Rc::clone(&windows))
            .with_clipboard(Rc::new(clipboard.clone()))
            .with_menu_bar(Rc::new(menu_bar.clone()))
            .with_popup_menus(Rc::new(popup_menus.clone()))
            .with_tray_icons(Rc::new(tray.clone()))
            .with_file_dialogs(Rc::new(file_dialogs.clone()));
        let compositor = Arc::new(TestCompositor {
            layers: Arc::default(),
            host: inner.dispatcher(),
        });
        let vsync = Arc::new(TestVsync {
            waiting: Mutex::default(),
            host: inner.dispatcher(),
        });
        DesktopPlatform {
            inner,
            windows,
            compositor,
            vsync,
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

/// The screen's vsyncs: every wait is served at the host's next turn, which the wait wakes the app
/// for, as a screen's refresh does.
pub(crate) struct TestVsync {
    waiting: Mutex<Vec<Weak<dyn VsyncObserver>>>,
    host: Arc<dyn Dispatcher>,
}

impl TestVsync {
    /// Whether a vsync is waited for.
    fn is_waited_for(&self) -> bool {
        !self.waiting.lock().unwrap().is_empty()
    }

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
        self.host.wake_now();
    }
}

/// Composition layers that show nothing: each counts the hardware buffers it is shown, keeps the
/// children each present showed, lets go of each buffer at once, and wakes the test's host, which
/// a test waiting for a present turns for.
pub(crate) struct TestCompositor {
    /// Each layer made, in order.
    layers: Arc<Mutex<Vec<Arc<TestLayer>>>>,
    host: Arc<dyn Dispatcher>,
}

impl TestCompositor {
    /// How many hardware buffers the layers have been shown.
    pub(crate) fn presents(&self) -> usize {
        self.layers
            .lock()
            .unwrap()
            .iter()
            .map(|layer| layer.presents.load(Ordering::SeqCst))
            .sum()
    }

    /// Whether each layer made, in order, was made opaque.
    pub(crate) fn opaque(&self) -> Vec<bool> {
        let layers = self.layers.lock().unwrap();
        layers.iter().map(|layer| layer.opaque).collect()
    }

    /// The children each present to the layer made `index`th showed, each by the order it was
    /// made in, with its placement.
    pub(crate) fn children_presented(&self, index: usize) -> Vec<Vec<(usize, Placement)>> {
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
            host: Arc::clone(&self.host),
        });
        self.layers.lock().unwrap().push(Arc::clone(&layer));
        layer
    }

    fn accepted_buffers(&self) -> waft::ui::AcceptedBuffers {
        waft::ui::AcceptedBuffers::default()
    }
}

/// The children one present showed, the bottom-most first.
type Children = Vec<(Arc<dyn CompositionLayer>, Placement)>;

struct TestLayer {
    opaque: bool,
    presents: AtomicUsize,
    /// The children each present showed, in order.
    children: Mutex<Vec<Children>>,
    host: Arc<dyn Dispatcher>,
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
        self.host.wake_now();
    }

    /// Nothing: each buffer is let go of as it is presented.
    fn current_picture(&self) -> Option<waft::ui::LayerPicture> {
        None
    }
}
