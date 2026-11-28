//! A desktop of the tests' own for WaOS's windows: Waft's test platform making a window for each
//! request at once, on one screen whose Dock keeps 70 points along its bottom, with the pointer
//! where the test puts it, the system's fonts, an opener that keeps what it is asked to open, a
//! tray that keeps its icons, file dialogs that answer what the test gives, and composition layers
//! that show nothing; and the
//! app as it starts, over a store in a folder of the test's own: the library, the programs'
//! windows, the questions they put to their user and the programs recording, on an engine whose
//! screens never refresh, which keeps the programs' days in the activity record, as the app's does. The test plays the host: it reports each window made, draws the frames
//! asked for, a twentieth of a second apart, runs the app's tasks, and taps and types as a user
//! does.

use crate::state::test_turns::{Turn, turn_until};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use waft::ui::{CompositionLayer, Compositor, HardwareBuffer, LayerPicture, Placement};
use waft::ui::{
    Contact, Dispatcher, EmbedderClient, Frame, KeyData, KeyEventDeviceType, KeyEventType, Offset,
    Platform, PlatformRef, PointerDeviceKind, PointerReport, Rect, ReportedChange, RequestId,
    Screen, ScreenId, Screens, TargetPlatform, TextAffinity, TextEditingValue, TextSelection,
    TrayIconEvent, TrayIconId, View as _, ViewError, ViewFocusDirection, ViewFocusEvent,
    ViewFocusState, ViewId, ViewRef, WindowConfig,
};
use waft::{
    AnyElement, App, AppLifecycleState, Curves, Entity, ScrollPositionAlignmentPolicy, Scrollable,
    Shell, State as _, Text, WidgetsBinding, downcast_widget, run_widget,
};
use waft_cupertino::install_cupertino_icon_font;
use waft_rendering::RenderEditable;
use waft_services::{LogicalKeyboardKey, PhysicalKeyboardKey, ServicesBinding};
#[cfg(target_os = "macos")]
use waft_test::Opened;
use waft_test::{
    MockFileDialogs, MockOpener, MockTrayIcons, ShownTrayIcon, TestPlatform, TestWindow,
    TestWindows,
};
use waft_winui::install_icon_font;
use waos_package::{Category, Manifest, SigningKey};
use waos_services::activity::ActivityRecord;

use super::{
    Library, LibraryDomains, ProgramsOpen, open_library, open_settings_when_asked,
    show_phone_files, work,
};
use crate::logic::opened_package::tests::{write_package, write_package_of, write_package_showing};
use crate::logic::package_copies::PackageCopies;
use crate::logic::phone_files::PhoneFilesAccess;
use crate::logic::phone_files::PhoneFilesOnRequest;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::preferences::PreferencesFile;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::prompts::{MachinePickerOnRequest, OutputPickerOnRequest, PromptsOnRequest};
use crate::logic::recording_indicator::RecordingIndicatorOnRequest;
use crate::logic::running_apps::RunningApps;
use crate::logic::startup::{self, Outside, Setup};
use crate::logic::test_engine;
use crate::logic::test_outputs::StatedOutputs;
use crate::state::ViewWindows;
use crate::state::phone_files::{follow_phone_files, phone_files_on_request};
use crate::state::questions::{self, Questions};
use crate::state::recordings::Recordings;
use crate::ui::every_window::{LibraryWindow, every_window, program_windows};
use waos_services::permissions::Prompts;

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
    pub(crate) activity: ActivityRecord,
    /// What the user lets each app use.
    pub(crate) permissions: waos_services::permissions::Permissions,
    pub(crate) runs: Arc<RunningApps>,
    /// What the engine asks the user through, for a test to ask as a program does.
    pub(crate) prompts: Arc<PromptsOnRequest>,
    /// What the engine shows its picker of outputs through, for a test to pick as a program does.
    pub(crate) output_picker: Arc<OutputPickerOnRequest>,
    /// What the engine shows its picker of machines through, for a test to pick as a program does.
    pub(crate) machine_picker: Arc<MachinePickerOnRequest>,
    /// What asks for the phone's files, on a desktop with a phone's.
    pub(crate) phone_files_on_request: Option<PhoneFilesOnRequest>,
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
        let work = work().unwrap();
        let setup = Setup::for_tests(folder.path(), work.handle().clone());
        let (started, (prompts, prompted, requests)) = startup::start(
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
                let faces = ProgramFaces::new(Arc::clone(before.packages), None);
                let (placement, requests) = WindowsOnRequest::new(faces);
                let shell = test_engine::shell(Arc::new(placement));
                let capabilities = waos_local::LocalCapabilities::new();
                let kept = (prompts, prompted, requests);
                let mut outside =
                    Outside::new(shell, test_engine::no_screens(), capabilities, kept);
                outside.audio = Some(Arc::new(StatedOutputs));
                Ok(outside)
            },
        )
        .expect("WaOS starts");
        let domains = LibraryDomains::of(&started);
        let startup::Started {
            running: runs,
            packages,
            permissions,
            activity,
            ..
        } = started;
        let asking = crate::logic::asking_now::AskingNow::new(permissions.asking());
        let output_picker = Arc::new(prompts.output_picker(Arc::clone(&asking)));
        let (phone_files_on_request, phone_files_followed) = (kind.phone_files.as_ref())
            .map(|access| phone_files_on_request(access.as_ref(), prompts.phone_files_off()))
            .unzip();
        let machine_picker =
            Arc::new(prompts.machine_picker(asking, phone_files_on_request.clone()));
        let (recording_indicator, recording_changes) = RecordingIndicatorOnRequest::new();
        let platform = Rc::new(LibraryPlatform::new(kind.system));
        let phone_files = kind.phone_files;
        let started = Rc::new(RefCell::new(None));
        let kept = Rc::clone(&started);
        let kept_activity = activity.clone();
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            let view_windows = ViewWindows::listen(app, requests);
            let prompt_windows = questions::prompt_windows(
                app,
                view_windows.clone(),
                ProgramFaces::new(Arc::clone(&packages), None),
            );
            let phone_files = phone_files
                .zip(phone_files_followed)
                .map(|(access, followed)| follow_phone_files(app, access, followed));
            Questions::listen(
                app,
                prompted.questions,
                prompt_windows.clone(),
                Arc::clone(&packages),
            );
            let recordings = Recordings::listen(app, recording_changes, Arc::clone(&packages));
            let library = open_library(
                app,
                domains,
                copies,
                ProgramsOpen::InWindows,
                preferences,
                work,
            );
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
            activity: kept_activity,
            permissions,
            runs,
            prompts,
            output_picker,
            machine_picker,
            phone_files_on_request,
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

    /// A package of `id` named `name` at 1.0.0, filed under `category`, whose icon is the PNG
    /// `icon`, for a picture of the library with apps as their authors make them.
    pub(crate) fn package_showing(
        &self,
        id: &str,
        name: &str,
        category: Option<Category>,
        icon: &[u8],
    ) -> std::path::PathBuf {
        let packages = self.folder.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        let manifest = Manifest::new(id.parse().unwrap(), name, "1.0.0".parse().unwrap())
            .unwrap()
            .with_category(category);
        write_package_showing(
            &packages,
            &manifest,
            crate::logic::test_installs::EMPTY_COMPONENT,
            icon,
            &self.key,
        )
    }

    /// A package of `id` named `name` at 1.0.0, filed under `category`.
    pub(crate) fn package_filed(
        &self,
        id: &str,
        name: &str,
        category: Category,
    ) -> std::path::PathBuf {
        let packages = self.folder.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        let manifest = Manifest::new(id.parse().unwrap(), name, "1.0.0".parse().unwrap())
            .unwrap()
            .with_category(Some(category));
        write_package_of(
            &packages,
            &manifest,
            crate::logic::test_installs::EMPTY_COMPONENT,
            &self.key,
        )
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

    /// Installs the test program at `component` as `id`, named `name`, at 1.0.0, from a package,
    /// its code compiled already (`logic::test_installs`), and frames until the library shows it.
    pub(crate) fn install_compiled(&mut self, id: &str, name: &str, component: &str) {
        let bytes = std::fs::read(component).unwrap();
        let package = self.package(id, name, "1.0.0", &bytes);
        let packages = self.read(|library| Arc::clone(&library.packages));
        crate::logic::test_installs::install_package(&packages, &package);
        let installed: waos_package::ProgramId = id.parse().unwrap();
        self.settle_until("the library shows the program", |desktop| {
            desktop.read(|library| library.installed_program(&installed).is_some())
        });
    }

    /// The version of the program `id` installed now.
    pub(crate) fn version_of(&self, id: &str) -> waos_services::packages::InstalledVersion {
        let packages = self.read(|library| Arc::clone(&library.packages));
        crate::logic::test_installs::installed_version(&packages, id)
    }

    /// The lock of the program `id`, held until the answer is dropped, as an open holds it from
    /// its load to its start: an install or removal of it waits meanwhile.
    pub(crate) fn hold_lock(&self, id: &str) -> waos_services::packages::AppLock {
        let id = id.parse().unwrap();
        let (runtime, packages) =
            self.read(|library| (library.runtime(), Arc::clone(&library.packages)));
        runtime.block_on(packages.lock(&id))
    }

    /// Frames until the package manager is done with every program, as after an install, its
    /// code compiled, and every removal the library asked for is done.
    pub(crate) fn settle_until_operations_end(&mut self) {
        self.settle_until("the package manager's work", |desktop| {
            desktop.read(Library::is_idle)
        });
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

    /// Backspace, pressed and released.
    pub(crate) fn press_backspace(&mut self) {
        self.press(
            PhysicalKeyboardKey::BACKSPACE,
            LogicalKeyboardKey::BACKSPACE,
        );
    }

    /// Return, pressed and released.
    pub(crate) fn press_enter(&mut self) {
        self.press(PhysicalKeyboardKey::ENTER, LogicalKeyboardKey::ENTER);
    }

    /// The key `physical`, meaning `logical`, pressed and released.
    pub(crate) fn press(&mut self, physical: PhysicalKeyboardKey, logical: LogicalKeyboardKey) {
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
        self.type_text_in_motion(view, text);
        self.settle();
    }

    /// Types as [`type_text`](Self::type_text) does, leaving what the typing starts in motion,
    /// for [`frames`](Self::frames) to show it move.
    pub(crate) fn type_text_in_motion(&mut self, view: ViewId, text: &str) {
        let typed = TextEditingValue::new()
            .text(text)
            .selection(TextSelection::collapsed(
                text.chars().count() as i32,
                TextAffinity::Downstream,
            ));
        self.shell.text_input_editing_value(view, typed);
    }

    /// Taps the label `text` shows in the window of `view`, with the mouse.
    pub(crate) fn tap(&mut self, view: ViewId, text: &str) {
        let at = self.label_at(text);
        self.click_at(view, at, 1);
    }

    /// Taps the label `text` shows in the window of `view`, with the mouse, leaving what the tap
    /// starts in motion, for [`frames`](Self::frames) to show it move.
    pub(crate) fn tap_in_motion(&mut self, view: ViewId, text: &str) {
        let at = self.label_at(text);
        self.press_at(view, at, 1);
    }

    /// Draws `count` frames, each [`FRAME_INTERVAL`] after the last.
    pub(crate) fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.pump();
        }
    }

    /// Swipes a finger from `from` to `to` in the window of `view`, a frame's move at a time over
    /// half a second, then lifts it, as a swipe across a phone's screen.
    pub(crate) fn swipe(&mut self, view: ViewId, from: Offset, to: Offset) {
        const MOVES: usize = 10;
        let touch = |desktop: &mut LibraryDesktop, at: Offset, change| {
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
        touch(self, from, ReportedChange::Add);
        touch(self, from, ReportedChange::Down { buttons: 1 });
        for step in 1..=MOVES {
            let at = from + (to - from) * (step as f64 / MOVES as f64);
            touch(self, at, ReportedChange::Move { buttons: 1 });
        }
        touch(self, to, ReportedChange::Up { buttons: 0 });
        touch(self, to, ReportedChange::Remove);
        self.settle();
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

    /// Clicks at `at` in the window of `view` with the mouse's `buttons`, then lets what it
    /// started settle.
    fn click_at(&mut self, view: ViewId, at: Offset, buttons: i64) {
        self.press_at(view, at, buttons);
        self.settle();
    }

    /// Clicks at `at` in the window of `view` with the mouse's `buttons`.
    fn press_at(&mut self, view: ViewId, at: Offset, buttons: i64) {
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
    }

    /// Moves the mouse to `at`, in the window of `view`, with no button down.
    pub(crate) fn hover_at(&mut self, view: ViewId, at: Offset) {
        self.shell.pointer_reports(vec![PointerReport {
            view_id: view,
            time_stamp: self.elapsed,
            device: 1,
            kind: PointerDeviceKind::Mouse,
            position: [at.dx(), at.dy()],
            change: ReportedChange::Hover { buttons: 0 },
            contact: Contact::default(),
        }]);
        self.pump();
        self.settle();
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

    /// Taps the label `text` nearest the height of the label `beside`, with the mouse: a row's
    /// button, by the name on its row, where several rows have a button saying the same.
    pub(crate) fn tap_beside_label(&mut self, view: ViewId, beside: &str, text: &str) {
        let row = self.label_at(beside).dy();
        let away = |at: &Offset| (at.dy() - row).abs();
        let at = self
            .every(text)
            .into_iter()
            .min_by(|one, other| away(one).total_cmp(&away(other)))
            .unwrap_or_else(|| panic!("no label {text:?} on screen: {:?}", self.labels()));
        self.click_at(view, at, 1);
    }

    /// Scrolls what the label `over` is in to its end, with the mouse's wheel.
    pub(crate) fn scroll_to_end(&mut self, view: ViewId, over: &str) {
        let at = self.label_at(over);
        self.wheel_at(view, at, 100_000.0);
        self.settle();
    }

    /// Taps the last of the labels showing `text`, the lowest on a page of sections that each
    /// have one, with the mouse.
    pub(crate) fn tap_last(&mut self, view: ViewId, text: &str) {
        let at = self
            .every(text)
            .into_iter()
            .max_by(|one, other| one.dy().total_cmp(&other.dy()))
            .unwrap_or_else(|| panic!("no label {text:?} on screen: {:?}", self.labels()));
        self.click_at(view, at, 1);
    }

    /// Where the middle of each label showing `text` is, in its view.
    pub(crate) fn every(&self, text: &str) -> Vec<Offset> {
        let mut app = self.shell.app();
        onstage_elements(&mut app)
            .into_iter()
            .filter(|element| label_of(&app, *element).as_deref() == Some(text))
            .filter_map(|element| {
                let label = element.render_object(&app)?.as_box()?;
                let size = label.size(&app);
                let middle = Offset::new(size.width() / 2.0, size.height() / 2.0);
                Some(label.local_to_global(&app, middle, None))
            })
            .collect()
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

    /// Whether the text field on screen draws its caret now: what its renderer is told, which
    /// follows the caret's blink.
    pub(crate) fn draws_a_caret(&self) -> bool {
        let mut app = self.shell.app();
        onstage_elements(&mut app).into_iter().any(|element| {
            element
                .render_object(&app)
                .and_then(|render| render.downcast::<RenderEditable>(&app))
                .is_some_and(|editable| *editable.show_cursor(&app).value(&app))
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

    /// Where the middle of the label showing `text` is, in its view.
    pub(crate) fn find(&self, text: &str) -> Option<Offset> {
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

    /// Turns of the host's loop until `done`, failing after `WITHIN`, and then until settled:
    /// a turn follows at once while the app has a frame, a window or a timer to turn for, and
    /// otherwise once a task wakes the app.
    pub(crate) fn settle_until(&mut self, what: &str, done: impl Fn(&LibraryDesktop) -> bool) {
        let platform = Rc::clone(&self.platform);
        turn_until(&platform.inner, what, || {
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
    /// window made that the app has not heard of, or a timer waiting on the desktop's clock.
    fn has_turns_to_take(&self) -> bool {
        !self.platform.inner.frame_requests().is_empty()
            || self.platform.windows.made().len() > self.reported_made
            || self
                .platform
                .inner
                .wakes()
                .last()
                .is_some_and(Option::is_some)
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

/// Every element mounted and onstage, as a finder skipping offstage widgets walks them: none in
/// an `Offstage`, and none of a route kept alive under an opaque one.
pub(crate) fn onstage_elements(app: &mut App) -> Vec<AnyElement> {
    let Some(root) = WidgetsBinding::instance(app).root_element(app) else {
        return Vec::new();
    };
    std::iter::once(root)
        .chain(root.descendants(app, true))
        .collect()
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
                .with_system_fonts()
                .with_gpu()
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

    fn open_image_codec(&self, bytes: Arc<[u8]>) -> waft::ui::ImageCodecFuture {
        self.inner.open_image_codec(bytes)
    }

    fn import_pixels(&self, pixels: waft::ui::valo::PixelBuffer) -> Option<waft::ui::Image> {
        self.inner.import_pixels(pixels)
    }

    fn picture_to_image_sync(
        &self,
        picture: &waft::ui::Picture,
        width: u32,
        height: u32,
    ) -> Option<waft::ui::Image> {
        self.inner.picture_to_image_sync(picture, width, height)
    }

    fn read_image_pixels(
        &self,
        image: &waft::ui::Image,
    ) -> waft::ui::image_codecs::ImageFuture<waft::ui::valo::PixelBuffer> {
        self.inner.read_image_pixels(image)
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

    fn accepted_buffers(&self) -> waft::ui::AcceptedBuffers {
        waft::ui::AcceptedBuffers::default()
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
