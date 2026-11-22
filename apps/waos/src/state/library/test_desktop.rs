//! A desktop of the tests' own for WaOS's windows: Waft's test platform making a window for each
//! request at once, on one screen whose Dock keeps 70 points along its bottom, with the pointer
//! where the test puts it, the system's fonts, an opener that keeps what it is asked to open, a
//! tray that keeps its icons, file dialogs that answer what the test gives, and composition layers
//! that show nothing; and the
//! app as it starts, over a store in a folder of the test's own: the library, the programs'
//! windows, the questions they put to their user and the programs recording, on an engine whose
//! screens never refresh, which keeps the programs' days in the store, as the app's does. The test plays the host: it reports each window made, draws the frames
//! asked for, a twentieth of a second apart, runs the app's tasks, and taps and types as a user
//! does.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use waft::ui::{CompositionLayer, Compositor, HardwareBuffer, LayerPicture, Placement};
use waft::ui::{
    Contact, Dispatcher, EmbedderClient, Frame, KeyData, KeyEventDeviceType, KeyEventType, Offset,
    Platform, PlatformRef, PointerDeviceKind, PointerReport, Rect, ReportedChange, RequestId,
    Screen, ScreenId, Screens, SystemFontSource, TargetPlatform, TextAffinity, TextEditingValue,
    TextSelection, TrayIconEvent, TrayIconId, View as _, ViewError, ViewFocusDirection,
    ViewFocusEvent, ViewFocusState, ViewId, ViewRef, WindowConfig,
};
use waft::{
    AnyElement, App, AppLifecycleState, Curves, Entity, Offstage, ScrollPositionAlignmentPolicy,
    Scrollable, Shell, State as _, Text, WidgetsBinding, downcast_widget, run_widget,
};
use waft_cupertino::install_cupertino_icon_font;
use waft_services::{LogicalKeyboardKey, PhysicalKeyboardKey, ServicesBinding};
#[cfg(target_os = "macos")]
use waft_test::Opened;
use waft_test::{
    MockFileDialogs, MockOpener, MockTrayIcons, ShownTrayIcon, TestPlatform, TestWindow,
    TestWindows,
};
use waft_winui::install_icon_font;
use waos_package::SigningKey;
use waos_store::{SourceProxy, Store};

use super::{Library, ProgramsOpen, open_library, open_settings_when_asked, show_phone_files};
use crate::logic::environments::ProgramEnvironments;
use crate::logic::installed_code::HeldCompiles;
use crate::logic::installed_runs::InstalledRuns;
use crate::logic::opened_package::tests::write_package;
use crate::logic::package_copies::PackageCopies;
use crate::logic::phone_files::PhoneFilesAccess;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::preferences::PreferencesFile;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
use crate::logic::prompts::{MachinePickerOnRequest, OutputPickerOnRequest, PromptsOnRequest};
use crate::logic::recording_indicator::RecordingIndicatorOnRequest;
use crate::logic::runs::Runs;
use crate::logic::test_engine;
use crate::logic::test_outputs::StatedOutputs;
use crate::state::ViewWindows;
use crate::state::phone_files::follow_phone_files;
use crate::state::questions::{self, Questions};
use crate::state::recordings::Recordings;
use crate::ui::every_window::{LibraryWindow, every_window, program_windows};

/// How long a test waits for the library's work.
const WITHIN: Duration = Duration::from_secs(60);

/// How far apart the frames are drawn.
const FRAME_INTERVAL: Duration = Duration::from_millis(50);

/// How many frames settle what a step started: a second's worth.
const SETTLING_FRAMES: usize = 20;

/// How many times [`LibraryDesktop::scroll_until_visible`] scrolls before it gives up: Flutter's
/// `scrollUntilVisible`'s default `maxScrolls`.
const MOST_SCROLLS: usize = 50;

/// The desktop's one screen.
pub(crate) const SCREEN: ScreenId = ScreenId(1);

/// The screen's frame, with a menu bar 33 points deep and a Dock keeping 70 along its bottom.
pub(crate) const SCREEN_FRAME: Rect = Rect::from_ltwh(0.0, 0.0, 1728.0, 1117.0);
pub(crate) const VISIBLE_FRAME: Rect = Rect::from_ltrb(0.0, 33.0, 1728.0, 1047.0);

pub(crate) struct LibraryDesktop {
    pub(crate) shell: Shell,
    platform: Rc<LibraryPlatform>,
    pub(crate) library: Entity<Library>,
    pub(crate) store: Store,
    pub(crate) runs: Arc<Runs>,
    /// What the engine asks the user through, for a test to ask as a program does.
    pub(crate) prompts: Arc<PromptsOnRequest>,
    /// What the engine shows its picker of outputs through, for a test to pick as a program does.
    pub(crate) output_picker: Arc<OutputPickerOnRequest>,
    /// What the engine shows its picker of machines through, for a test to pick as a program does.
    pub(crate) machine_picker: Arc<MachinePickerOnRequest>,
    /// What the engine tells who records, for a test to tell as the engine does.
    pub(crate) recording_indicator: Arc<RecordingIndicatorOnRequest>,
    pub(crate) folder: TempDir,
    key: SigningKey,
    /// How many of the windows made the app has heard of.
    reported_made: usize,
    /// Whether the mouse was added, as a host adds it before its first event.
    mouse_added: bool,
    /// The time of the frame drawn last.
    elapsed: Duration,
}

/// What a desktop of the tests' own is: the system it plays, whether WaOS keeps its own
/// preferences there, in the test's folder, and whether it reaches the phone's files only as the
/// user allows, as on Android.
#[derive(Clone)]
pub(crate) struct DesktopKind {
    pub(crate) system: TargetPlatform,
    pub(crate) own_preferences: bool,
    pub(crate) phone_files: Option<Rc<dyn PhoneFilesAccess>>,
}

impl Default for DesktopKind {
    /// macOS, with no preferences of WaOS's own, reaching what its user does.
    fn default() -> DesktopKind {
        DesktopKind {
            system: TargetPlatform::MacOS,
            own_preferences: false,
            phone_files: None,
        }
    }
}

/// What a test shows the library in: the window made of the library and the programs recording,
/// where it makes one, and what it answers besides.
pub(crate) type ShowLibrary<T> = dyn FnOnce(
    &mut App,
    &Entity<Library>,
    &Entity<Recordings>,
    &Entity<ViewWindows>,
) -> (Option<LibraryWindow>, T);

impl LibraryDesktop {
    /// The desktop, macOS's, with the library and the programs recording in the window `show`
    /// makes of them, where it makes one, with the programs' windows, and with what `show` answers
    /// besides.
    pub(crate) fn new<T: 'static>(
        show: impl FnOnce(
            &mut App,
            &Entity<Library>,
            &Entity<Recordings>,
            &Entity<ViewWindows>,
        ) -> (Option<LibraryWindow>, T)
        + 'static,
    ) -> (LibraryDesktop, T) {
        LibraryDesktop::of(DesktopKind::default(), Box::new(show))
    }

    /// The desktop `kind` says, with the library shown as `show` shows it.
    pub(crate) fn of<T: 'static>(
        kind: DesktopKind,
        show: Box<ShowLibrary<T>>,
    ) -> (LibraryDesktop, T) {
        let folder = tempfile::tempdir().unwrap();
        let preferences = kind
            .own_preferences
            .then(|| PreferencesFile::at(folder.path().join("preferences.toml")));
        let copies = PackageCopies::clear(folder.path().join("package-copies")).unwrap();
        let runs = Arc::new(Runs::default());
        let store = Store::open(&folder.path().join("store"), SourceProxy::None).expect("a store");
        let (placement, requests) = WindowsOnRequest::new(ProgramFaces::new(store.clone(), None));
        let (prompts, prompted) = PromptsOnRequest::new(crate::logic::asking_now::AskingNow::new());
        let output_picker = Arc::new(prompts.output_picker());
        let machine_picker = Arc::new(prompts.machine_picker());
        let (recording_indicator, recording_changes) = RecordingIndicatorOnRequest::new();
        let prompts = Arc::new(prompts);
        let engine = test_engine::engine_keeping_days(
            Arc::new(placement),
            Arc::clone(&prompts) as Arc<dyn waos::Prompts>,
            Arc::new(store.clone()),
            &folder.path().join("code"),
        );
        let installed_runs = InstalledRuns::new(
            Arc::new(engine),
            ProgramEnvironments::with_audio(
                waos_local::LocalCapabilities::new(),
                Some(Arc::new(StatedOutputs)),
                store.clone(),
            ),
            Arc::clone(&runs),
            ProgramPlaces::new(ProgramPlace::Windows),
        );
        let platform = Rc::new(LibraryPlatform::new(kind.system));
        let phone_files = kind.phone_files;
        let started = Rc::new(RefCell::new(None));
        let kept = Rc::clone(&started);
        let kept_store = store.clone();
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            let view_windows = ViewWindows::listen(app, requests);
            let prompt_windows = questions::prompt_windows(
                app,
                view_windows.clone(),
                ProgramFaces::new(store.clone(), None),
            );
            let phone_files = phone_files.map(|access| follow_phone_files(app, access));
            Questions::listen(
                app,
                prompted.questions,
                prompt_windows.clone(),
                store.clone(),
                phone_files.clone(),
            );
            let recordings = Recordings::listen(app, recording_changes, store.clone());
            let library = open_library(
                app,
                store,
                installed_runs,
                copies,
                ProgramsOpen::InWindows,
                preferences,
            )
            .expect("a library");
            open_settings_when_asked(app, &library, prompted.settings);
            if let Some(phone_files) = phone_files {
                show_phone_files(app, &library, phone_files);
            }
            let (library_window, shown) = show(app, &library, &recordings, &view_windows);
            let program_windows = program_windows(&view_windows, &prompt_windows);
            run_widget(
                app,
                every_window(library_window, program_windows, prompt_windows, None),
            );
            install_icon_font(app);
            install_cupertino_icon_font(app);
            *kept.borrow_mut() = Some((library, shown));
        });
        let (library, shown) = started.borrow_mut().take().expect("the app started");
        let mut desktop = LibraryDesktop {
            shell,
            platform,
            library,
            store: kept_store,
            runs,
            prompts,
            output_picker,
            machine_picker,
            recording_indicator: Arc::new(recording_indicator),
            folder,
            key: SigningKey::generate(),
            reported_made: 0,
            mouse_added: false,
            elapsed: Duration::ZERO,
        };
        desktop.settle();
        (desktop, shown)
    }

    /// A package of `id` at `version`, named `name`, made of `component`, signed with the test's
    /// key and written to the test's folder.
    pub(crate) fn package(
        &self,
        id: &str,
        name: &str,
        version: &str,
        component: &[u8],
    ) -> std::path::PathBuf {
        let packages = self.folder.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        write_package(&packages, id, name, version, component, &self.key)
    }

    /// WaOS goes to the background and comes back to the front, as the user leaving it for the
    /// system's settings and coming back does, then the frames it asks for.
    pub(crate) fn leave_and_come_back(&mut self) {
        for state in [AppLifecycleState::Paused, AppLifecycleState::Resumed] {
            let mut app = self.shell.app();
            ServicesBinding::instance(&mut app).handle_lifecycle_message(&mut app, state);
        }
        self.shell.cell().checkpoint();
        self.settle();
    }

    /// `act` on the library, then the frames it asks for.
    pub(crate) fn update(&mut self, act: impl FnOnce(&mut Library, &mut waft::Context<Library>)) {
        self.library.update(&mut self.shell.app(), act);
        self.shell.cell().checkpoint();
        self.settle();
    }

    pub(crate) fn read<R>(&self, read: impl FnOnce(&Library) -> R) -> R {
        read(self.library.read(&self.shell.app()))
    }

    /// Frames until no installed program's code is being compiled, as after an install.
    pub(crate) fn settle_until_code_is_ready(&mut self) {
        self.settle_until("the programs' code", |desktop| {
            desktop.read(|library| {
                library
                    .installed()
                    .iter()
                    .all(|program| !library.is_compiling(program.manifest.id()))
            })
        });
    }

    /// Holds the library's compiles back until the answer is dropped.
    pub(crate) fn hold_compiles(&self) -> HeldCompiles {
        self.read(|library| library.runs.code.hold())
    }

    /// The windows made so far, in the order they were asked for.
    pub(crate) fn windows(&self) -> Vec<Rc<TestWindow>> {
        self.platform.windows.made()
    }

    /// The icons in the system's tray now, in the order added.
    pub(crate) fn tray_icons(&self) -> Vec<ShownTrayIcon> {
        self.platform.tray.icons()
    }

    /// The tray icon `icon` was clicked, as the host reports it.
    pub(crate) fn click_tray_icon(&mut self, icon: TrayIconId) {
        let at = Offset::new(1000.0, 10.0);
        self.shell
            .tray_icon_event(icon, TrayIconEvent::Activated { position: at });
        self.settle();
    }

    /// What the app asked the system's opener to open, in order.
    #[cfg(target_os = "macos")]
    pub(crate) fn opened(&self) -> Vec<Opened> {
        self.platform.opener.asked()
    }

    /// The system's file dialogs, which cancel each dialog until the test gives an answer.
    pub(crate) fn file_dialogs(&self) -> MockFileDialogs {
        self.platform.file_dialogs.clone()
    }

    /// The pointer is at `pointer` from now on.
    pub(crate) fn move_pointer(&self, pointer: Option<Offset>) {
        self.platform.pointer.set(pointer);
    }

    /// The window of `view` gained or lost the focus, as the host reports it.
    pub(crate) fn focus(&mut self, view: ViewId, focused: bool) {
        let state = if focused {
            ViewFocusState::Focused
        } else {
            ViewFocusState::Unfocused
        };
        self.shell.view_focus_changed(ViewFocusEvent {
            view_id: view,
            state,
            direction: ViewFocusDirection::Undefined,
        });
        self.settle();
    }

    /// The user asked to close the window of `view`, as with the system's close command.
    pub(crate) fn request_close(&mut self, view: ViewId) {
        self.shell.view_close_requested(view);
        self.settle();
    }

    /// Escape, pressed and released.
    pub(crate) fn press_escape(&mut self) {
        self.press(PhysicalKeyboardKey::ESCAPE, LogicalKeyboardKey::ESCAPE);
    }

    /// Return, pressed and released.
    pub(crate) fn press_enter(&mut self) {
        self.press(PhysicalKeyboardKey::ENTER, LogicalKeyboardKey::ENTER);
    }

    /// The key `physical`, meaning `logical`, pressed and released.
    fn press(&mut self, physical: PhysicalKeyboardKey, logical: LogicalKeyboardKey) {
        for event_type in [KeyEventType::Down, KeyEventType::Up] {
            self.shell.key_data(KeyData {
                time_stamp: self.elapsed,
                event_type,
                device_type: KeyEventDeviceType::Keyboard,
                physical: physical.usb_hid_usage,
                logical: logical.key_id,
                character: None,
                synthesized: false,
            });
        }
        self.settle();
    }

    /// The keyboard makes what is edited in the window of `view` read `text`, the caret after it,
    /// as typing it does.
    pub(crate) fn type_text(&mut self, view: ViewId, text: &str) {
        let typed = TextEditingValue::new()
            .text(text)
            .selection(TextSelection::collapsed(
                text.chars().count() as i32,
                TextAffinity::Downstream,
            ));
        self.shell.text_input_editing_value(view, typed);
        self.settle();
    }

    /// Taps the label `text` shows in the window of `view`, with the mouse.
    pub(crate) fn tap(&mut self, view: ViewId, text: &str) {
        let at = self.label_at(text);
        self.click_at(view, at, 1);
    }

    /// Clicks the label `text` shows in the window of `view` with the mouse's secondary button, as
    /// a right click does.
    pub(crate) fn right_click(&mut self, view: ViewId, text: &str) {
        let at = self.label_at(text);
        self.click_at(view, at, 2);
    }

    /// Taps the middle of the first widget of type `W` on screen that `which` picks, with the
    /// mouse: for a button with no label, as an icon's.
    pub(crate) fn tap_widget<W: 'static>(&mut self, view: ViewId, which: impl Fn(&W) -> bool) {
        let at = self
            .find_widget(&which)
            .unwrap_or_else(|| panic!("no such widget on screen: {:?}", self.labels()));
        self.click_at(view, at, 1);
    }

    /// Holds a finger on the label `text` shows in the window of `view` for a second, then lifts
    /// it, as a long press on a phone.
    pub(crate) fn long_press(&mut self, view: ViewId, text: &str) {
        let at = self.label_at(text);
        let touch = |desktop: &mut LibraryDesktop, change| {
            desktop.shell.pointer_reports(vec![PointerReport {
                view_id: view,
                time_stamp: desktop.elapsed,
                device: 2,
                kind: PointerDeviceKind::Touch,
                position: [at.dx(), at.dy()],
                change,
                contact: Contact::default(),
            }]);
            desktop.pump();
        };
        touch(self, ReportedChange::Add);
        touch(self, ReportedChange::Down { buttons: 1 });
        self.settle();
        touch(self, ReportedChange::Up { buttons: 0 });
        touch(self, ReportedChange::Remove);
        self.settle();
    }

    fn label_at(&self, text: &str) -> Offset {
        self.find(text)
            .unwrap_or_else(|| panic!("no label {text:?} on screen: {:?}", self.labels()))
    }

    /// Clicks at `at` in the window of `view` with the mouse's `buttons`.
    fn click_at(&mut self, view: ViewId, at: Offset, buttons: i64) {
        let mut changes = vec![
            ReportedChange::Hover { buttons: 0 },
            ReportedChange::Down { buttons },
            ReportedChange::Up { buttons: 0 },
        ];
        if !std::mem::replace(&mut self.mouse_added, true) {
            changes.insert(0, ReportedChange::Add);
        }
        for change in changes {
            self.shell.pointer_reports(vec![PointerReport {
                view_id: view,
                time_stamp: self.elapsed,
                device: 1,
                kind: PointerDeviceKind::Mouse,
                position: [at.dx(), at.dy()],
                change,
                contact: Contact::default(),
            }]);
            self.pump();
        }
        self.settle();
    }

    /// Scrolls what holds the label `text` shows, in the window of `view`, to its end, with the
    /// mouse's wheel over the label.
    pub(crate) fn scroll_to_end_at(&mut self, view: ViewId, text: &str) {
        let at = self.label_at(text);
        self.wheel_at(view, at, 100_000.0);
    }

    /// Turns the mouse's wheel at `at`, in the window of `view`, by `delta` pixels, a positive
    /// one towards the content's end.
    fn wheel_at(&mut self, view: ViewId, at: Offset, delta: f64) {
        let mut changes = vec![
            ReportedChange::Hover { buttons: 0 },
            ReportedChange::Scroll {
                delta: [0.0, delta],
            },
        ];
        if !std::mem::replace(&mut self.mouse_added, true) {
            changes.insert(0, ReportedChange::Add);
        }
        for change in changes {
            self.shell.pointer_reports(vec![PointerReport {
                view_id: view,
                time_stamp: self.elapsed,
                device: 1,
                kind: PointerDeviceKind::Mouse,
                position: [at.dx(), at.dy()],
                change,
                contact: Contact::default(),
            }]);
            self.pump();
        }
        self.settle();
    }

    /// Scrolls what holds the label `over` shows, in the window of `view`, by `delta` pixels at a
    /// time with the mouse's wheel where `over` was, a positive `delta` towards its end, until the
    /// label showing `text` is in what scrolls, at most [`MOST_SCROLLS`] times; then brings that
    /// label whole into view: Flutter's `WidgetController.scrollUntilVisible` (`flutter_test`'s
    /// `controller.dart`), a wheel where it drags, its finder's `skipOffstage` the label in the
    /// scrollable's viewport, and ending in `Scrollable.ensureVisible` as its `dragUntilVisible`
    /// does, but by the least scroll that shows the label whole, where Flutter's aligns it to the
    /// top, beneath a Cupertino page's bar.
    pub(crate) fn scroll_until_visible(
        &mut self,
        view: ViewId,
        text: &str,
        over: &str,
        delta: f64,
    ) {
        let at = self.label_at(over);
        let mut scrolls = MOST_SCROLLS;
        while scrolls > 0 && !self.in_its_viewport(text) {
            self.wheel_at(view, at, delta);
            scrolls -= 1;
        }
        let label = self
            .label_element(text)
            .filter(|_| self.in_its_viewport(text))
            .unwrap_or_else(|| panic!("no label {text:?} after scrolling: {:?}", self.labels()));
        let policy = if delta > 0.0 {
            ScrollPositionAlignmentPolicy::KeepVisibleAtEnd
        } else {
            ScrollPositionAlignmentPolicy::KeepVisibleAtStart
        };
        let brought = Scrollable::ensure_visible(
            &mut self.shell.app(),
            label,
            0.0,
            Duration::ZERO,
            Curves::linear(),
            policy,
        );
        drop(brought);
        self.settle();
    }

    /// Whether the label showing `text` is built and overlaps the viewport of what scrolls it.
    fn in_its_viewport(&self, text: &str) -> bool {
        let Some(label) = self.label_element(text) else {
            return false;
        };
        let mut app = self.shell.app();
        let Some(scrollable) = Scrollable::maybe_of(&mut app, label, None) else {
            return true;
        };
        let (Some(label), Some(viewport)) =
            (bounds(&app, label), bounds(&app, scrollable.context(&app)))
        else {
            return false;
        };
        label.bottom > viewport.top && label.top < viewport.bottom
    }

    /// Whether a widget of type `W` is on screen.
    pub(crate) fn has<W: 'static>(&self) -> bool {
        let mut app = self.shell.app();
        onstage_elements(&mut app)
            .into_iter()
            .any(|element| downcast_widget::<W>(element.widget(&app).as_ref()).is_some())
    }

    /// How many widgets of type `W` that `which` picks are on screen.
    pub(crate) fn count<W: 'static>(&self, which: impl Fn(&W) -> bool) -> usize {
        let mut app = self.shell.app();
        onstage_elements(&mut app)
            .into_iter()
            .filter(|element| {
                downcast_widget::<W>(element.widget(&app).as_ref()).is_some_and(&which)
            })
            .count()
    }

    /// Where the middle of the first widget of type `W` that `which` picks is, in its view.
    fn find_widget<W: 'static>(&self, which: &impl Fn(&W) -> bool) -> Option<Offset> {
        self.bounds_of(which).map(|bounds| bounds.center())
    }

    /// Where the first widget of type `W` that `which` picks is, in its view.
    pub(crate) fn bounds_of<W: 'static>(&self, which: &impl Fn(&W) -> bool) -> Option<Rect> {
        let mut app = self.shell.app();
        let element = onstage_elements(&mut app).into_iter().find(|element| {
            downcast_widget::<W>(element.widget(&app).as_ref()).is_some_and(which)
        })?;
        bounds(&app, element)
    }

    /// The first widget of type `W` level with the label `text`, as a row's switch is with its
    /// name.
    pub(crate) fn widget_beside<W: Clone + 'static>(&self, text: &str) -> Option<W> {
        self.beside::<W>(text).map(|(widget, _)| widget)
    }

    /// Where the first widget of type `W` level with the label `text` is, in its view.
    pub(crate) fn bounds_beside<W: Clone + 'static>(&self, text: &str) -> Option<Rect> {
        self.beside::<W>(text).map(|(_, bounds)| bounds)
    }

    /// Taps the middle of the first widget of type `W` level with the label `text`, with the
    /// mouse.
    pub(crate) fn tap_beside<W: Clone + 'static>(&mut self, view: ViewId, text: &str) {
        let bounds = self
            .bounds_beside::<W>(text)
            .unwrap_or_else(|| panic!("nothing beside {text:?}: {:?}", self.labels()));
        self.click_at(view, bounds.center(), 1);
    }

    /// The first widget of type `W` whose height spans the middle of the label `text`, and where
    /// it is.
    fn beside<W: Clone + 'static>(&self, text: &str) -> Option<(W, Rect)> {
        let middle = self.find(text)?.dy();
        let mut app = self.shell.app();
        onstage_elements(&mut app).into_iter().find_map(|element| {
            let widget = downcast_widget::<W>(element.widget(&app).as_ref())?.clone();
            let bounds = bounds(&app, element)?;
            (bounds.top <= middle && middle <= bounds.bottom).then_some((widget, bounds))
        })
    }

    /// Whether a label shows `text`.
    pub(crate) fn shows(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    /// Every label shown, for a failure to say what there is.
    pub(crate) fn labels(&self) -> Vec<String> {
        let mut app = self.shell.app();
        onstage_elements(&mut app)
            .into_iter()
            .filter_map(|element| label_of(&app, element))
            .collect()
    }

    /// The label showing `text`, built, on screen or in what a list builds around it.
    fn label_element(&self, text: &str) -> Option<AnyElement> {
        let mut app = self.shell.app();
        onstage_elements(&mut app)
            .into_iter()
            .find(|element| label_of(&app, *element).as_deref() == Some(text))
    }

    /// Where the label showing `text` is, in its view.
    fn find(&self, text: &str) -> Option<Offset> {
        let element = self.label_element(text)?;
        let app = self.shell.app();
        let label = element.render_object(&app)?.as_box()?;
        let size = label.size(&app);
        Some(label.local_to_global(
            &app,
            Offset::new(size.width() / 2.0, size.height() / 2.0),
            None,
        ))
    }

    /// Frames until what the last step started has settled.
    pub(crate) fn settle(&mut self) {
        for _ in 0..SETTLING_FRAMES {
            self.pump();
        }
    }

    /// Frames until `done`, failing after [`WITHIN`].
    pub(crate) fn settle_until(&mut self, what: &str, done: impl Fn(&LibraryDesktop) -> bool) {
        let deadline = Instant::now() + WITHIN;
        while !done(self) {
            assert!(Instant::now() < deadline, "{what} never happened");
            thread::sleep(Duration::from_millis(5));
            self.pump();
        }
        self.settle();
    }

    /// One turn of the host's loop: the windows made are reported, the frames asked for drawn,
    /// and the app's tasks run.
    fn pump(&mut self) {
        let made = self.platform.windows.made();
        for window in &made[self.reported_made..] {
            self.shell.view_added(window.test_view().id());
        }
        self.reported_made = made.len();
        self.elapsed += FRAME_INTERVAL;
        let views = self.platform.inner.take_frame_requests();
        if !views.is_empty() {
            self.shell.frame(
                &views,
                Frame {
                    elapsed: self.elapsed,
                    vsync: None,
                },
            );
        }
        self.shell.wake(self.elapsed);
    }
}

/// Every element mounted and not offstage.
pub(crate) fn onstage_elements(app: &mut App) -> Vec<AnyElement> {
    let Some(root) = WidgetsBinding::instance(app).root_element(app) else {
        return Vec::new();
    };
    let mut elements = vec![root];
    let mut index = 0;
    while index < elements.len() {
        let element = elements[index];
        let offstage = downcast_widget::<Offstage>(element.widget(app).as_ref())
            .is_some_and(|widget| widget.offstage);
        if !offstage {
            elements.extend(element.children(app));
        }
        index += 1;
    }
    elements
}

/// Where `element` is drawn, in its view, when it is a box.
fn bounds(app: &App, element: AnyElement) -> Option<Rect> {
    let found = element.render_object(app)?.as_box()?;
    let size = found.size(app);
    let top_left = found.local_to_global(app, Offset::new(0.0, 0.0), None);
    Some(Rect::from_ltwh(
        top_left.dx(),
        top_left.dy(),
        size.width(),
        size.height(),
    ))
}

/// The text `element` shows, when it is a `Text` of plain text.
pub(crate) fn label_of(app: &App, element: AnyElement) -> Option<String> {
    downcast_widget::<Text>(element.widget(app).as_ref())?
        .data
        .clone()
}

/// Waft's test platform, with windows made on request on its one screen, the screen's Dock and
/// the pointer, the system's fonts, an opener and file dialogs.
struct LibraryPlatform {
    inner: TestPlatform,
    windows: Rc<TestWindows>,
    pointer: Cell<Option<Offset>>,
    /// What the opener was asked, which only macOS's tests read: there alone does the library open
    /// the system's settings.
    #[cfg(target_os = "macos")]
    opener: MockOpener,
    tray: MockTrayIcons,
    file_dialogs: MockFileDialogs,
}

impl LibraryPlatform {
    fn new(system: TargetPlatform) -> LibraryPlatform {
        let windows = Rc::new(TestWindows::new().on_screen(SCREEN));
        let opener = MockOpener::new();
        let tray = MockTrayIcons::new();
        let file_dialogs = MockFileDialogs::new();
        LibraryPlatform {
            inner: TestPlatform::new()
                .on(system)
                .with_windows(Rc::clone(&windows))
                .with_font_source(|| Box::new(SystemFontSource::new(valo_fontmgr::platform())))
                .with_opener(Rc::new(opener.clone()))
                .with_tray_icons(Rc::new(tray.clone()))
                .with_file_dialogs(Rc::new(file_dialogs.clone())),
            windows,
            pointer: Cell::new(None),
            #[cfg(target_os = "macos")]
            opener,
            tray,
            file_dialogs,
        }
    }
}

impl Platform for LibraryPlatform {
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
        Some(Arc::new(NoComposition))
    }

    fn font_source(&self) -> Option<Box<dyn waft::ui::FontSource>> {
        self.inner.font_source()
    }

    fn tray_icons(&self) -> Option<&dyn waft::ui::TrayIcons> {
        self.inner.tray_icons()
    }

    fn opener(&self) -> Option<&dyn waft::ui::Opener> {
        self.inner.opener()
    }

    fn file_dialogs(&self) -> Option<&dyn waft::ui::FileDialogs> {
        self.inner.file_dialogs()
    }

    fn screens(&self) -> Option<&dyn Screens> {
        Some(self)
    }
}

impl Screens for LibraryPlatform {
    fn screens(&self) -> Vec<Screen> {
        vec![Screen {
            id: SCREEN,
            name: "Test Display".to_owned(),
            frame: SCREEN_FRAME,
            visible_frame: VISIBLE_FRAME,
            scale: 1.0,
            refresh_rate: Some(60.0),
        }]
    }

    fn pointer_location(&self) -> Option<Offset> {
        self.pointer.get()
    }
}

/// Composition layers that show nothing: each hardware buffer is let go of as it is presented.
struct NoComposition;

impl Compositor for NoComposition {
    fn make_layer(&self, _opaque: bool) -> Arc<dyn CompositionLayer> {
        Arc::new(NoComposition)
    }
}

impl CompositionLayer for NoComposition {
    fn present(
        &self,
        buffer: Arc<dyn HardwareBuffer>,
        _children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        drop(buffer);
        released();
    }

    fn current_picture(&self) -> Option<LayerPicture> {
        None
    }
}
