//! A phone of the tests' own: Waft's test platform giving the app one view, a phone's screen held
//! upright at three pixels a point with a status bar and a home indicator, with a compositor whose
//! layers show nothing and the system's fonts; the app's library and deck over a store in a folder
//! of the test's own; and an engine that refuses every view request, as a phone's does, whose
//! screens never refresh. The test plays the host and the user: it draws the frames asked for,
//! sixty a second on the test's own clock, runs the app's tasks, and puts fingers on the screen.
//!
//! The invariant: nothing a test sees depends on how long the machine takes to draw a frame. Time
//! is the test's clock, but for one thing a gesture reads from the real one: Waft's velocity
//! tracker takes a finger to have stopped when its last move is more than 40 ms of real time
//! old, and a frame drawn between a finger's last move and its lift can take longer than that on
//! a busy machine. So a lift is reported right behind a repeat of the finger's last move, as a
//! touch screen reports them, and a finger that stops before it lifts says so in moves that go
//! nowhere (`drag_and_let_go`).
//!
//! What a test reads is what a user would see: the labels on screen and where they are, and where
//! the last frame placed each program's picture, the composition layer of its tab's canvas; and
//! which program's canvas a touch reaches, and where on it, as the hit test of a finger coming
//! down finds it.

use crate::state::test_turns::{Turn, turn_until};
use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use waft::ui::valo::Op;
use waft::ui::{
    Contact, EmbedderClient, Frame, HostWindow, PlatformRef, PointerDeviceKind, PointerReport,
    ReportedChange, Scene, TargetPlatform, TextAffinity, TextEditingValue, TextSelection, View,
    ViewHost, ViewId, ViewMetrics, ViewPadding, ViewRef, WindowConfig, flattened_ops, slices,
};
use waft::{
    AnyElement, App, Entity, Offset, Opacity, Rect, Shell, TextStyle, WidgetsBinding,
    downcast_widget, run_widget,
};
use waft_cupertino::install_cupertino_icon_font;
use waft_gestures::{GestureBinding, HitTestResult};
use waft_rendering::{AnyRenderBox, BoxHitTestEntry};
use waft_test::{
    BarsShown, MockCompositor, TestPlatform, TestView, TestWindow, TestWindows, TextInputCall,
};
use waft_winui::install_icon_font;
use waos::{ProgramEnd, ProgramView, ViewPlacement};
use waos_package::{ProgramId, SigningKey};
use waos_program_window::{ProgramCanvas, WindowRestoration};

use super::tabs::{TabContent, TabStage, Tabs};
use crate::logic::opened_package::tests::write_package;
use crate::logic::package_copies::PackageCopies;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::preferences::PreferencesFile;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_places::{PlacedWhereOpened, ProgramPlace, TabsOnRequest};
use crate::logic::prompts::PromptsOnRequest;
use crate::logic::recording_indicator::RecordingIndicatorOnRequest;
use crate::logic::running_apps::RunningApps;
use crate::logic::startup::{self, Outside, Setup};
use crate::logic::test_engine::{self, NoWindows};
use crate::state::ViewWindows;
use crate::state::library::test_desktop::label_of;
use crate::state::library::{Library, LibraryDomains, ProgramsOpen, open_library, work};
use crate::state::questions::{self, Questions};
use crate::state::recordings::Recordings;
use crate::state::{bring_opened_programs_forward, phone_deck};
use crate::ui::every_window::{every_window, program_windows};
use waos_services::permissions::Prompts;

/// How far apart the frames are drawn, on the test's clock: a screen's sixty a second.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// How many frames in a row must ask for no other before the deck counts as settled, and how many
/// a settling may take at most.
const QUIET_FRAMES: usize = 6;
const MOST_SETTLING_FRAMES: usize = 900;

/// The number of a window the system brings back, out of the way of the screen's and the windows
/// made at requests.
const RESTORED_VIEW: u64 = 500;

/// The screen, in points, and what the system covers of it at the top and the bottom.
pub(crate) const SCREEN: [f64; 2] = [390.0, 844.0];
pub(crate) const STATUS_BAR: f64 = 47.0;
pub(crate) const HOME_INDICATOR: f64 = 34.0;
const PIXEL_RATIO: f64 = 3.0;

pub(crate) const TRIANGLE: &str = "dev.example.triangle";
pub(crate) const SURVEY: &str = "dev.example.survey";
pub(crate) const TRAP: &str = "dev.example.trap";
pub(crate) const TWO_WINDOWS: &str = "dev.example.twowindows";
pub(crate) const BACK: &str = "dev.example.back";
pub(crate) const INCOMPATIBLE: &str = "dev.example.incompatible";

/// The phone's screen: a test view whose size, and what the system covers of it and takes swipes
/// on, the test sets.
pub(crate) struct PhoneScreen {
    view: TestView,
    /// In points: the system's bars at the top and the bottom, and the keyboard.
    top_padding: Cell<f64>,
    bottom_padding: Cell<f64>,
    keyboard: Cell<f64>,
    /// In points: the system's gesture zone along the bottom, none as on an iPhone.
    bottom_gesture_inset: Cell<f64>,
}

impl PhoneScreen {
    fn new() -> PhoneScreen {
        PhoneScreen {
            view: TestView::with_pixel_ratio(
                SCREEN[0] * PIXEL_RATIO,
                SCREEN[1] * PIXEL_RATIO,
                PIXEL_RATIO,
            ),
            top_padding: Cell::new(STATUS_BAR),
            bottom_padding: Cell::new(HOME_INDICATOR),
            keyboard: Cell::new(0.0),
            bottom_gesture_inset: Cell::new(0.0),
        }
    }

    fn last_scene(&self) -> Option<Arc<Scene>> {
        self.view.last_scene()
    }
}

impl View for PhoneScreen {
    fn id(&self) -> ViewId {
        self.view.id()
    }

    /// The test view's, with the system's bars as padding, less what the keyboard covers of the
    /// bottom one, as a phone reports them, and the system's gesture zone along the bottom.
    fn metrics(&self) -> ViewMetrics {
        let in_pixels = |points: f64| points * PIXEL_RATIO;
        let (top, bottom, keyboard) = (
            in_pixels(self.top_padding.get()),
            in_pixels(self.bottom_padding.get()),
            in_pixels(self.keyboard.get()),
        );
        let covered = |bottom: f64| ViewPadding {
            left: 0.0,
            top,
            right: 0.0,
            bottom,
        };
        ViewMetrics {
            padding: covered((bottom - keyboard).max(0.0)),
            view_padding: covered(bottom),
            view_insets: ViewPadding {
                bottom: keyboard,
                ..ViewPadding::ZERO
            },
            system_gesture_insets: ViewPadding {
                bottom: in_pixels(self.bottom_gesture_inset.get()),
                ..ViewPadding::ZERO
            },
            ..self.view.metrics()
        }
    }

    fn present(&self, scene: Arc<Scene>) {
        self.view.present(scene);
    }

    fn host(&self) -> &dyn ViewHost {
        self.view.host()
    }
}

/// Where a frame placed a program's picture, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Picture {
    /// The whole picture's box.
    pub(crate) bounds: Rect,
    /// What of the box the clips above it let show.
    pub(crate) visible: Rect,
    /// How large it is drawn, of the size its program draws it at.
    pub(crate) scale: f64,
}

/// A program's canvas a touch reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TouchedCanvas {
    /// Where the canvas is drawn, in points.
    pub(crate) drawn: Rect,
    /// Where on the canvas the touch is, in the canvas's own points as it is laid out: where the
    /// program hears it.
    pub(crate) at: Offset,
}

/// How a test phone shows its programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhoneKind {
    /// Inside WaOS, in the deck's tabs, as an iPhone does and `waos phone` on a desktop.
    Tabs,
    /// Each in windows of its own, as Android's tasks are: Waft's test platform makes a window
    /// for each request at once, the deck in WaOS's own view, and the user's choice of tabs kept
    /// in a preferences file of the test's own.
    Tasks,
}

pub(crate) struct TestPhone {
    pub(crate) shell: Shell,
    platform: Rc<TestPlatform>,
    /// The windows made at the programs' requests, on a phone of tasks.
    pub(crate) windows: Rc<TestWindows>,
    /// How many of the windows made the app has heard of.
    reported_made: usize,
    pub(crate) screen: Rc<PhoneScreen>,
    compositor: MockCompositor,
    pub(crate) library: Entity<Library>,
    pub(crate) tabs: Entity<Tabs>,
    /// The programs' windows, on a phone of tasks.
    pub(crate) view_windows: Entity<ViewWindows>,
    pub(crate) runs: Arc<RunningApps>,
    /// What installs the library's apps.
    packages: Arc<waos_services::packages::PackageManager>,
    /// Where the programs' questions to their user are put.
    pub(crate) prompts: Arc<PromptsOnRequest>,
    pub(crate) folder: TempDir,
    pub(crate) key: SigningKey,
    /// The time of the frame drawn last.
    elapsed: Duration,
    /// The fingers on the screen, by their number, each with whether the host has reported it
    /// before.
    fingers_added: Vec<i64>,
    /// Where each finger that has moved since it came down last moved to.
    last_moves: Vec<FingerMove>,
}

/// A move of a finger's, as it was reported.
#[derive(Clone, Copy)]
struct FingerMove {
    finger: i64,
    to: Offset,
    time_stamp: Duration,
}

impl TestPhone {
    /// A phone that shows its programs in the deck's tabs.
    pub(crate) fn new() -> TestPhone {
        TestPhone::of(PhoneKind::Tabs)
    }

    /// A phone of `kind`.
    pub(crate) fn of(kind: PhoneKind) -> TestPhone {
        let folder = tempfile::tempdir().unwrap();
        let copies = PackageCopies::clear(folder.path().join("package-copies")).unwrap();
        let (usual, programs_open) = match kind {
            PhoneKind::Tabs => (ProgramPlace::Tab, ProgramsOpen::InTabs),
            PhoneKind::Tasks => (ProgramPlace::Windows, ProgramsOpen::AsChosen),
        };
        let preferences = PreferencesFile::at(folder.path().join("preferences.toml"));
        let work = work().unwrap();
        let setup = Setup {
            usual_place: usual,
            ..Setup::for_tests(folder.path())
        };
        let ((started, (prompts, prompted, requests)), tab_requests) = {
            let (tabs_on_request, tab_requests) = TabsOnRequest::new();
            let started = startup::start(
                setup,
                |running| {
                    let (prompts, prompted) = PromptsOnRequest::new(Arc::clone(running));
                    let prompts = Arc::new(prompts);
                    (
                        Arc::clone(&prompts) as Arc<dyn Prompts>,
                        (prompts, prompted),
                    )
                },
                |before, (prompts, prompted)| {
                    let (own_windows, requests) =
                        WindowsOnRequest::new(ProgramFaces::new(Arc::clone(before.packages), None));
                    let windows: Arc<dyn ViewPlacement> = match kind {
                        PhoneKind::Tabs => Arc::new(NoWindows),
                        PhoneKind::Tasks => Arc::new(own_windows),
                    };
                    let placement = PlacedWhereOpened {
                        places: before.places.clone(),
                        tabs: tabs_on_request,
                        windows,
                    };
                    let shell = test_engine::shell(Arc::new(placement));
                    let capabilities = waos_local::LocalCapabilities::new();
                    let platform = test_engine::no_screens();
                    let kept = (prompts, prompted, requests);
                    Ok(Outside::new(shell, platform, capabilities, kept))
                },
            )
            .expect("WaOS starts");
            (started, tab_requests)
        };
        let domains = LibraryDomains::of(&started);
        let startup::Started {
            running: runs,
            packages,
            ..
        } = started;
        let screen = Rc::new(PhoneScreen::new());
        let compositor = MockCompositor::new();
        let windows = Rc::new(TestWindows::new());
        let mut platform = TestPlatform::new()
            .with_view(Rc::clone(&screen) as ViewRef)
            .with_compositor(Arc::new(compositor.clone()))
            .with_system_fonts();
        if kind == PhoneKind::Tasks {
            platform = platform
                .on(TargetPlatform::Android)
                .with_windows(Rc::clone(&windows));
        }
        let platform = Rc::new(platform);
        let (_indicator, recording_changes) = RecordingIndicatorOnRequest::new();
        let deck_runs = Arc::clone(&runs);
        let served_packages = Arc::clone(&packages);
        let started = Rc::new(std::cell::RefCell::new(None));
        let kept = Rc::clone(&started);
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            let view_windows = ViewWindows::listen(app, requests);
            let prompt_windows = questions::prompt_windows(
                app,
                view_windows.clone(),
                ProgramFaces::new(Arc::clone(&served_packages), None),
            );
            Questions::listen(
                app,
                prompted.questions,
                prompt_windows.clone(),
                Arc::clone(&served_packages),
            );
            let recordings = Recordings::listen(app, recording_changes, served_packages);
            let library =
                open_library(app, domains, copies, programs_open, Some(preferences), work);
            bring_opened_programs_forward(app, &library, &view_windows);
            let tabs = Tabs::listen(app, deck_runs, tab_requests);
            let deck = phone_deck(
                app,
                library.clone(),
                recordings,
                tabs.clone(),
                &view_windows,
            );
            let program_windows = program_windows(&view_windows, &prompt_windows);
            run_widget(
                app,
                every_window(Some(deck), program_windows, prompt_windows, None),
            );
            install_icon_font(app);
            install_cupertino_icon_font(app);
            *kept.borrow_mut() = Some((library, tabs, view_windows));
        });
        let (library, tabs, view_windows) = started.borrow_mut().take().expect("the app started");
        let mut phone = TestPhone {
            shell,
            platform,
            windows,
            reported_made: 0,
            screen,
            compositor,
            library,
            tabs,
            view_windows,
            runs,
            packages,
            prompts,
            folder,
            key: SigningKey::generate(),
            elapsed: Duration::ZERO,
            fingers_added: Vec::new(),
            last_moves: Vec::new(),
        };
        phone.settle();
        phone
    }

    /// The phone with `programs` installed, each a test program's component under an id and a
    /// name.
    pub(crate) fn with_installed(programs: &[(&str, &str, &str)]) -> TestPhone {
        let mut phone = TestPhone::new();
        for (component, id, name) in programs {
            phone.install(component, id, name);
        }
        phone
    }

    /// Installs the test program at `component` as `id`, named `name`, from a package, its code
    /// compiled already (`logic::test_installs`), and waits until the library shows it.
    pub(crate) fn install(&mut self, component: &str, id: &str, name: &str) {
        let packages = self.folder.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        let bytes = std::fs::read(component).unwrap();
        let file = write_package(&packages, id, name, "1.0.0", &bytes, &self.key);
        crate::logic::test_installs::install_package(&self.packages, &file);
        let installed: ProgramId = id.parse().unwrap();
        self.settle_until("the library shows the program", |phone| {
            let app = phone.shell.app();
            let library = phone.library.read(&app);
            library
                .installed()
                .iter()
                .any(|program| program.manifest.id() == &installed)
        });
    }

    /// Opens the program `id` through the library, as a tap on its icon does, and waits for
    /// nothing.
    pub(crate) fn start_opening(&mut self, id: &str) {
        let id: ProgramId = id.parse().unwrap();
        self.library
            .update(&mut self.shell.app(), |library, cx| library.open(&id, cx));
    }

    /// Opens the program `id` through the library, and waits until it runs in its tab and the
    /// deck has settled on it.
    pub(crate) fn open(&mut self, id: &str) {
        self.start_opening(id);
        self.settle_until("the program runs in its tab", |phone| {
            phone.stage(id) == Some("running")
        });
    }

    /// Sends the deck to the page of `id`'s tab, or to the home's, and lets it settle.
    pub(crate) fn show(&mut self, id: Option<&str>) {
        let id: Option<ProgramId> = id.map(|id| id.parse().unwrap());
        self.tabs.update(&mut self.shell.app(), |tabs, cx| {
            match id.and_then(|id| tabs.list().tab_of(&id)) {
                Some(tab) => tabs.show_page(tab, cx),
                None => {
                    tabs.go_home(cx);
                }
            }
        });
        self.settle();
    }

    /// Closes the tab of `id`, as the library's Close on its icon does, and lets the deck settle.
    pub(crate) fn close(&mut self, id: &str) {
        let id: ProgramId = id.parse().unwrap();
        self.tabs.update(&mut self.shell.app(), |tabs, cx| {
            tabs.close_program(&id, cx)
        });
        self.settle();
    }

    /// Closes the tab at `index`, as its card's close in the overview does, and lets the deck
    /// settle.
    pub(crate) fn close_tab(&mut self, index: usize) {
        self.tabs.update(&mut self.shell.app(), |tabs, cx| {
            let tab = tabs.list().tabs()[index].id;
            tabs.close(tab, cx);
        });
        self.settle();
    }

    /// What `read` answers of the stage of `id`'s tab; none without a tab.
    fn read_stage<R>(&self, id: &str, read: impl FnOnce(&TabStage) -> R) -> Option<R> {
        let app = self.shell.app();
        let tabs = self.tabs.read(&app);
        tabs.tabs()
            .find_map(|tab| match tab.content {
                TabContent::Program { id: program, stage } if program.as_str() == id => Some(stage),
                _ => None,
            })
            .map(read)
    }

    /// How far the tab of `id` got: starting, running or ended; none without a tab.
    pub(crate) fn stage(&self, id: &str) -> Option<&'static str> {
        self.read_stage(id, |stage| match stage {
            TabStage::Starting => "starting",
            TabStage::Waiting => "waiting",
            TabStage::Running(_) => "running",
            TabStage::Ended { .. } => "ended",
        })
    }

    /// How the program of `id`'s tab ended; none while it runs, or without a tab.
    pub(crate) fn end(&self, id: &str) -> Option<ProgramEnd> {
        self.read_stage(id, |stage| match stage {
            TabStage::Ended { end, .. } => Some(end.clone()),
            _ => None,
        })
        .flatten()
    }

    /// What each tab is, in order: the home, or its program's id.
    pub(crate) fn tab_kinds(&self) -> Vec<String> {
        let app = self.shell.app();
        let tabs = self.tabs.read(&app);
        tabs.tabs()
            .map(|tab| match tab.content {
                TabContent::Home => "home".to_owned(),
                TabContent::Program { id, .. } => id.to_string(),
            })
            .collect()
    }

    /// What the current tab is: the home, or its program's id.
    pub(crate) fn current_tab(&self) -> String {
        let app = self.shell.app();
        let tabs = self.tabs.read(&app);
        let current = tabs.list().current();
        let index = tabs.list().index_of(current).expect("it is open");
        drop(app);
        self.tab_kinds().swap_remove(index)
    }

    /// Whether the program of `id`'s tab runs and goes back within itself, as its tab last heard.
    pub(crate) fn handles_back(&self, id: &str) -> bool {
        let app = self.shell.app();
        let tabs = self.tabs.read(&app);
        tabs.tabs().any(|tab| match tab.content {
            TabContent::Program {
                id: program,
                stage: TabStage::Running(shown),
            } => program.as_str() == id && shown.read(&app).handles_back(),
            _ => false,
        })
    }

    pub(crate) fn overview_open(&self) -> bool {
        let app = self.shell.app();
        self.tabs.read(&app).list().overview_open()
    }

    /// The program `id` running, and its one view.
    pub(crate) fn program_view(&self, id: &str) -> ProgramView {
        let id: ProgramId = id.parse().unwrap();
        let program = self.runs.running(&id).expect("it runs");
        let views = program.views();
        assert_eq!(views.len(), 1, "the tab's view, and no other");
        views.into_iter().next().unwrap()
    }

    /// Each time the system's bars over the phone's screen were hidden or shown again, oldest
    /// first.
    pub(crate) fn bars_shown(&self) -> Vec<BarsShown> {
        self.screen.view.test_host().bars_shown()
    }

    /// Whether a widget of type `W` is on screen.
    pub(crate) fn has<W: 'static>(&self) -> bool {
        let mut app = self.shell.app();
        elements_in_sight(&mut app)
            .into_iter()
            .any(|element| downcast_widget::<W>(element.widget(&app).as_ref()).is_some())
    }

    /// Where the first widget of type `W` on screen that `which` picks is.
    pub(crate) fn bounds_of<W: 'static>(&self, which: impl Fn(&W) -> bool) -> Option<Rect> {
        let mut app = self.shell.app();
        let element = elements_in_sight(&mut app).into_iter().find(|element| {
            downcast_widget::<W>(element.widget(&app).as_ref()).is_some_and(&which)
        })?;
        let found = element.render_object(&app)?.as_box()?;
        let size = found.size(&app);
        let top_left = found.local_to_global(&app, Offset::ZERO, None);
        let bottom_right =
            found.local_to_global(&app, Offset::new(size.width(), size.height()), None);
        Some(Rect::from_points(top_left, bottom_right))
    }

    /// The program's canvas a touch at `at` reaches, and where on it, as hit testing finds it
    /// when the finger comes down; none where it reaches no canvas.
    pub(crate) fn canvas_touched_at(&self, at: Offset) -> Option<TouchedCanvas> {
        let mut app = self.shell.app();
        let canvases: Vec<AnyRenderBox> = elements_in_sight(&mut app)
            .into_iter()
            .filter(|element| {
                downcast_widget::<ProgramCanvas>(element.widget(&app).as_ref()).is_some()
            })
            .filter_map(|element| element.render_object(&app)?.as_box())
            .collect();
        let mut result = HitTestResult::new();
        GestureBinding::instance(&mut app).hit_test_in_view(
            &mut app,
            &mut result,
            at,
            self.screen.id(),
        );
        let reached = result.path().iter().find_map(|entry| {
            let target: &dyn Any = entry.target();
            let hit = target.downcast_ref::<BoxHitTestEntry>()?;
            canvases.contains(&hit.target()).then_some(*hit)
        })?;
        let canvas = reached.target();
        let size = canvas.size(&app);
        let top_left = canvas.local_to_global(&app, Offset::ZERO, None);
        let bottom_right =
            canvas.local_to_global(&app, Offset::new(size.width(), size.height()), None);
        Some(TouchedCanvas {
            drawn: Rect::from_points(top_left, bottom_right),
            at: reached.local_position(),
        })
    }

    /// Whether what has the keyboard's focus is a program's canvas.
    pub(crate) fn keyboard_is_a_programs(&self) -> bool {
        let mut app = self.shell.app();
        let Some(context) = waft::primary_focus(&mut app).and_then(|node| node.context(&app))
        else {
            return false;
        };
        let mut in_a_canvas = false;
        context.visit_ancestor_elements(&app, &mut |ancestor| {
            in_a_canvas =
                downcast_widget::<ProgramCanvas>(ancestor.widget(&app).as_ref()).is_some();
            !in_a_canvas
        });
        in_a_canvas
    }

    /// Every label shown.
    pub(crate) fn labels(&self) -> Vec<String> {
        let mut app = self.shell.app();
        elements_in_sight(&mut app)
            .into_iter()
            .filter_map(|element| label_of(&app, element))
            .collect()
    }

    pub(crate) fn shows(&self, text: &str) -> bool {
        self.labels().iter().any(|label| label == text)
    }

    /// The style the label showing `text` is drawn in, as it was built; none where no label on
    /// screen shows it.
    pub(crate) fn label_style(&self, text: &str) -> Option<TextStyle> {
        let mut app = self.shell.app();
        elements_in_sight(&mut app).into_iter().find_map(|element| {
            let widget = element.widget(&app);
            let label = downcast_widget::<waft::Text>(widget.as_ref())?;
            (label.data.as_deref() == Some(text)).then(|| label.style.clone().unwrap_or_default())
        })
    }

    /// Where the label showing `text` is on screen, as it is drawn: moved and scaled with what
    /// holds it.
    pub(crate) fn label_bounds(&self, text: &str) -> Option<Rect> {
        self.bounds_of::<waft::Text>(|label| label.data.as_deref() == Some(text))
    }

    /// Where the last frame placed the picture of the `nth` program opened, counting from 0, in
    /// points; none where the frame placed it nowhere.
    pub(crate) fn picture(&self, nth: usize) -> Option<Picture> {
        let layer = Arc::clone(self.compositor.made().get(nth)?);
        let scene = self.screen.last_scene()?;
        let slices = slices(&scene, |_| None);
        let (_, placement) = slices
            .composition_layers()
            .find(|(placed, _)| std::ptr::addr_eq(Arc::as_ptr(placed), Arc::as_ptr(&layer)))?;
        let in_points = |rect: Rect| {
            Rect::from_ltrb(
                rect.left / PIXEL_RATIO,
                rect.top / PIXEL_RATIO,
                rect.right / PIXEL_RATIO,
                rect.bottom / PIXEL_RATIO,
            )
        };
        Some(Picture {
            bounds: in_points(placement.bounds()),
            visible: in_points(placement.visible_bounds()),
            scale: placement.scale() / PIXEL_RATIO,
        })
    }

    /// The programs whose pictures the last frame placed, each by its place among the programs
    /// opened, counting from 0, in the order painted: the one beneath every other first.
    pub(crate) fn pictures_in_paint_order(&self) -> Vec<usize> {
        let Some(scene) = self.screen.last_scene() else {
            return Vec::new();
        };
        let made = self.compositor.made();
        slices(&scene, |_| None)
            .composition_layers()
            .filter_map(|(placed, _)| {
                made.iter()
                    .position(|made| std::ptr::addr_eq(Arc::as_ptr(made), Arc::as_ptr(placed)))
            })
            .collect()
    }

    /// The size of each clip the last frame draws, in the order drawn and in the coordinates each
    /// is recorded in: every clip in the picture a host draws of the frame, the drawing between
    /// its composition layers with the room made for each.
    pub(crate) fn clips_drawn(&self) -> Vec<[f64; 2]> {
        let Some(scene) = self.screen.last_scene() else {
            return Vec::new();
        };
        let picture = slices(&scene, |_| None).picture_with_room();
        flattened_ops(&picture)
            .iter()
            .filter_map(|op| match op {
                Op::ClipPath { path, .. } => {
                    let bounds = path.bounds();
                    Some([f64::from(bounds.width), f64::from(bounds.height)])
                }
                _ => None,
            })
            .collect()
    }

    /// Whether the app has asked for the keyboard, and not sent it away since.
    pub(crate) fn keyboard_asked_for(&self) -> bool {
        let calls = self.screen.view.test_host().text_input_calls();
        let last = calls
            .iter()
            .rev()
            .find(|call| matches!(call, TextInputCall::Show | TextInputCall::Hide));
        matches!(last, Some(TextInputCall::Show))
    }

    /// The keyboard makes what is edited read `text`, the caret after it, as typing it does.
    pub(crate) fn type_text(&mut self, text: &str) {
        let typed = TextEditingValue::new()
            .text(text)
            .selection(TextSelection::collapsed(
                text.chars().count() as i32,
                TextAffinity::Downstream,
            ));
        self.shell.text_input_editing_value(self.screen.id(), typed);
        self.settle();
    }

    /// The system covers `top` and `bottom` points of the screen from now on, and the keyboard
    /// `keyboard` points from its bottom.
    pub(crate) fn cover(&mut self, top: f64, bottom: f64, keyboard: f64) {
        self.screen.top_padding.set(top);
        self.screen.bottom_padding.set(bottom);
        self.screen.keyboard.set(keyboard);
        self.shell.view_metrics_changed(self.screen.id());
    }

    /// The system takes swipes that start in the `bottom` points along the screen's bottom from
    /// now on, as Android's gesture navigation does.
    pub(crate) fn take_swipes_along_the_bottom(&mut self, bottom: f64) {
        self.screen.bottom_gesture_inset.set(bottom);
        self.shell.view_metrics_changed(self.screen.id());
    }

    /// The screen measures `width` by `height` points from now on, as a phone turned does.
    pub(crate) fn resize(&mut self, width: f64, height: f64) {
        self.screen.view.set_physical_size(waft::ui::Size::new(
            width * PIXEL_RATIO,
            height * PIXEL_RATIO,
        ));
        self.shell.view_metrics_changed(self.screen.id());
    }

    /// The system's Back, as Waft's Android host sends it for the back button and the back
    /// gesture, and a frame is drawn: answers whether the app handled it, where the host would
    /// otherwise hand it to Android.
    pub(crate) fn back(&mut self) -> bool {
        self.back_in(self.screen.id())
    }

    /// The user's Back in `view`: answers whether the app handled it.
    pub(crate) fn back_in(&mut self, view: ViewId) -> bool {
        let handled = self.shell.pop_route(view);
        self.pump();
        handled
    }

    /// Every view of the program `id` running, in the order they arrived.
    pub(crate) fn program_views(&self, id: &str) -> Vec<ProgramView> {
        let id: ProgramId = id.parse().unwrap();
        let program = self.runs.running(&id).expect("it runs");
        program.views()
    }

    /// Whether programs opened from now on show inside WaOS, as the library says.
    pub(crate) fn programs_inside_waos(&self) -> Option<bool> {
        self.library.read(&self.shell.app()).programs_inside_waos()
    }

    /// Turns of the host's loop until the program `id`, started, has stood still once its start
    /// is over: a program that reads its views at its start has read them, so a view taken away
    /// after is one it heard of.
    pub(crate) fn wait_until_running_still(&mut self, id: &str) {
        self.pump_for(Duration::from_millis(500));
        let program_id: ProgramId = id.parse().unwrap();
        let program = self.runs.running(&program_id).expect("it runs");
        futures::executor::block_on(program.idle());
    }

    /// Whether the program `id` runs.
    pub(crate) fn runs(&self, id: &str) -> bool {
        let id: ProgramId = id.parse().unwrap();
        self.runs.running(&id).is_some()
    }

    /// The windows made at the program `id`'s requests and still open, on a phone of tasks, in
    /// the order they were asked for.
    pub(crate) fn windows_of(&self, id: &str) -> Vec<Rc<TestWindow>> {
        self.windows
            .made()
            .into_iter()
            .filter(|window| window.closes() == 0)
            .filter(|window| {
                let data = window.test_view().initial_data().unwrap_or_default();
                WindowRestoration::of(&data).is_some_and(|named| named.program == id)
            })
            .collect()
    }

    /// The system brings back a window whose view carries `initial_data`, as Android brings back
    /// a task from Recents: the window, its view given to the app, which hears of it.
    pub(crate) fn restore_window(&mut self, initial_data: &str) -> Rc<TestWindow> {
        let [width, height] = SCREEN.map(|points| points * PIXEL_RATIO);
        let view = TestView::with_pixel_ratio(width, height, PIXEL_RATIO)
            .with_id(ViewId(RESTORED_VIEW))
            .with_initial_data(initial_data);
        let window = TestWindow::around(view, WindowConfig::default());
        self.platform
            .give_view(Rc::clone(window.test_view()) as ViewRef);
        self.shell.view_added(ViewId(RESTORED_VIEW));
        self.pump();
        window
    }

    /// The user swipes `window`'s card away, as Android's Recents removes its task: the window is
    /// closed and its view goes.
    pub(crate) fn swipe_away(&mut self, window: &TestWindow) {
        let view = window.test_view().id();
        HostWindow::close(window);
        self.platform.take_view(view);
        self.shell.view_removed(view);
        self.pump();
    }

    /// The system says the window of `view` was minimized or restored, covered or shown.
    pub(crate) fn report_window_changed(&mut self, view: ViewId) {
        self.shell.window_changed(view);
        self.pump();
    }

    /// A finger taps `view` at `at`, in its points.
    pub(crate) fn tap_in(&mut self, view: ViewId, at: Offset) {
        for change in [
            ReportedChange::Add,
            ReportedChange::Down { buttons: 1 },
            ReportedChange::Up { buttons: 0 },
            ReportedChange::Remove,
        ] {
            self.report_in(view, self.elapsed, 1, at, change);
        }
        self.pump();
    }

    /// A finger, `finger`, comes down at `at`, in points, and a frame is drawn.
    pub(crate) fn touch_down(&mut self, finger: i64, at: Offset) {
        if !self.fingers_added.contains(&finger) {
            self.fingers_added.push(finger);
            self.report(finger, at, ReportedChange::Add);
        }
        self.report(finger, at, ReportedChange::Down { buttons: 1 });
        self.pump();
    }

    /// The finger `finger` moves to `to`, and a frame is drawn.
    pub(crate) fn touch_move(&mut self, finger: i64, to: Offset) {
        let moved = FingerMove {
            finger,
            to,
            time_stamp: self.elapsed,
        };
        self.last_moves.retain(|last| last.finger != finger);
        self.last_moves.push(moved);
        self.report_move(moved);
        self.pump();
    }

    /// The finger `finger` lifts at `at`, right behind a repeat of its last move, and a frame is
    /// drawn.
    pub(crate) fn touch_up(&mut self, finger: i64, at: Offset) {
        let last_move = self.last_moves.iter().find(|last| last.finger == finger);
        if let Some(last_move) = last_move.copied() {
            self.report_move(last_move);
        }
        self.report(finger, at, ReportedChange::Up { buttons: 0 });
        self.report(finger, at, ReportedChange::Remove);
        self.fingers_added.retain(|added| *added != finger);
        self.last_moves.retain(|last| last.finger != finger);
        self.pump();
    }

    /// The finger `finger` moves from `from` to `to` in a straight line over `frames` frames,
    /// staying down.
    pub(crate) fn slide(&mut self, finger: i64, from: Offset, to: Offset, frames: u32) {
        for frame in 1..=frames {
            let along = f64::from(frame) / f64::from(frames);
            self.touch_move(finger, from + (to - from) * along);
        }
    }

    /// A finger comes down at `from`, moves to `to` over `frames` frames and lifts there while
    /// still moving: a swipe, and a flick when the frames are few.
    pub(crate) fn swipe(&mut self, from: Offset, to: Offset, frames: u32) {
        self.touch_down(1, from);
        self.slide(1, from, to, frames);
        self.touch_up(1, to);
    }

    /// A finger comes down at `from`, moves to `to` over `frames` frames, rests there, and lifts:
    /// a drag let go with no speed.
    pub(crate) fn drag_and_let_go(&mut self, from: Offset, to: Offset, frames: u32) {
        self.touch_down(1, from);
        self.slide(1, from, to, frames);
        for _ in 0..12 {
            self.touch_move(1, to);
        }
        self.touch_up(1, to);
    }

    /// A finger comes down at `at` and lifts there: a tap.
    pub(crate) fn tap(&mut self, at: Offset) {
        self.touch_down(1, at);
        self.touch_up(1, at);
    }

    /// Reports `change` of `finger` at `at`, at the time of the frame drawn last.
    fn report(&mut self, finger: i64, at: Offset, change: ReportedChange) {
        self.report_at(self.elapsed, finger, at, change);
    }

    fn report_move(&mut self, moved: FingerMove) {
        let change = ReportedChange::Move { buttons: 1 };
        self.report_at(moved.time_stamp, moved.finger, moved.to, change);
    }

    fn report_at(&mut self, time_stamp: Duration, finger: i64, at: Offset, change: ReportedChange) {
        self.report_in(self.screen.id(), time_stamp, finger, at, change);
    }

    fn report_in(
        &mut self,
        view: ViewId,
        time_stamp: Duration,
        finger: i64,
        at: Offset,
        change: ReportedChange,
    ) {
        self.shell.pointer_reports(vec![PointerReport {
            view_id: view,
            time_stamp,
            device: finger,
            kind: PointerDeviceKind::Touch,
            position: [at.dx() * PIXEL_RATIO, at.dy() * PIXEL_RATIO],
            change,
            contact: Contact::default(),
        }]);
    }

    /// Frames until the deck asks for no more: what the last step started has settled.
    pub(crate) fn settle(&mut self) {
        let mut quiet = 0;
        for _ in 0..MOST_SETTLING_FRAMES {
            quiet = if self.pump() { 0 } else { quiet + 1 };
            if quiet >= QUIET_FRAMES {
                return;
            }
        }
    }

    /// Turns of the host's loop until `done`, failing after `WITHIN`, and then until the deck
    /// has settled: a turn follows at once while the app has a frame, a window or a timer to
    /// turn for, and otherwise once a task wakes the app.
    pub(crate) fn settle_until(&mut self, what: &str, done: impl Fn(&TestPhone) -> bool) {
        let platform = Rc::clone(&self.platform);
        turn_until(&platform, what, || {
            self.pump();
            if done(self) {
                Turn::Done
            } else if self.has_turns_to_take() {
                Turn::FrameAsked
            } else {
                Turn::Idle
            }
        });
        self.settle();
    }

    /// Whether the host's loop has something to turn for without a wake: a frame asked for, a
    /// window made that the app has not heard of, or a timer waiting on the phone's clock.
    fn has_turns_to_take(&self) -> bool {
        !self.platform.frame_requests().is_empty()
            || self.windows.made().len() > self.reported_made
            || self.platform.wakes().last().is_some_and(Option::is_some)
    }

    /// Turns of the host's loop for `time` of the wall's clock, whatever happens in it: for a
    /// check that a program did nothing in that time, which it says nowhere.
    pub(crate) fn pump_for(&mut self, time: Duration) {
        let until = Instant::now() + time;
        while Instant::now() < until {
            #[allow(
                clippy::disallowed_methods,
                reason = "real time for a program to go wrong in; exact once it says its start is over"
            )]
            thread::sleep(Duration::from_millis(5));
            self.pump();
        }
    }

    /// `frames` turns of the host's loop.
    pub(crate) fn pump_frames(&mut self, frames: u32) {
        for _ in 0..frames {
            self.pump();
        }
    }

    /// One turn of the host's loop: the frame asked for is drawn, and the app's tasks run.
    /// Whether a frame was asked for.
    pub(crate) fn pump(&mut self) -> bool {
        let made = self.windows.made();
        for window in &made[self.reported_made..] {
            self.shell.view_added(window.test_view().id());
        }
        self.reported_made = made.len();
        self.elapsed += FRAME_INTERVAL;
        let views = self.platform.take_frame_requests();
        let asked = !views.is_empty();
        if asked {
            self.shell.frame(
                &views,
                Frame {
                    elapsed: self.elapsed,
                    vsync: None,
                },
            );
        }
        self.shell.wake(self.elapsed);
        asked
    }
}

/// Every element mounted that a user could see: onstage, as a finder skipping offstage widgets
/// walks them, and none drawn at no opacity.
fn elements_in_sight(app: &mut App) -> Vec<AnyElement> {
    let Some(root) = WidgetsBinding::instance(app).root_element(app) else {
        return Vec::new();
    };
    let mut elements = vec![root];
    let mut index = 0;
    while index < elements.len() {
        let element = elements[index];
        let widget = element.widget(app);
        let hidden =
            downcast_widget::<Opacity>(widget.as_ref()).is_some_and(|widget| widget.opacity == 0.0);
        if !hidden {
            element.debug_visit_onstage_children(app, &mut |child| elements.push(child));
        }
        index += 1;
    }
    elements
}

impl Drop for TestPhone {
    fn drop(&mut self) {
        futures::executor::block_on(self.runs.stop_all());
    }
}

/// The phone, with triangle installed and opened in its tab, running.
pub(crate) fn with_triangle_running() -> TestPhone {
    let mut phone =
        TestPhone::with_installed(&[(waos_test_programs::TRIANGLE, TRIANGLE, "Triangle")]);
    phone.open(TRIANGLE);
    phone
}
