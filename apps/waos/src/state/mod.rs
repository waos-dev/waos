//! What ties the app to Waft's application: the engine started with what Waft reports of the
//! machine and kept current with it, each view a program asks for in a window of its own on a
//! desktop, and on a phone each program in a tab of the deck, which refuses every request, Waft's
//! clipboard as the programs' one, Waft's menus as theirs, a program's menu bar, context menus,
//! file dialogs and notifications its app's on macOS, WaOS's own menu bar, Waft's opener,
//! notifications elsewhere, players, reading of media and cameras as theirs, the dev server listening for `cargo waos`, the installed programs' shortcuts on Android, the library over the
//! store in the desktop's launcher or in a window of a phone's size, the questions
//! programs put to their user, the picker of their sound's output among them, shown on macOS in
//! dialogs of the programs' apps (`prompt_dialogs`), the programs recording, and the quit that
//! asks every program first.
//!
//! The invariant: nothing here is touched off the main thread. What reaches it from the programs'
//! threads and the dev server's comes through a queue a task of the main thread's empties: the view
//! requests (`logic::placement`), the questions (`logic::prompts`), who records
//! (`logic::recording_indicator`), the clipboard's calls (`logic::clipboard_calls`), the menus'
//! calls (`logic::menu_calls`), the openings (`logic::opener_calls`), the notifications' calls
//! (`logic::notification_calls`), the players' calls (`logic::media_player_calls`), the media
//! assets' calls (`logic::media_asset_calls`), the cameras'
//! calls (`logic::camera_calls`), each view's calls (`waos-program-window`'s), and on macOS what
//! the programs' apps send (`logic::program_apps`).

pub(crate) mod cameras;
pub(crate) mod clipboard;
pub(crate) mod deck;
#[cfg(not(target_os = "android"))]
pub(crate) mod desktop_host;
pub(crate) mod file_dialogs;
pub(crate) mod given_views;
pub(crate) mod image_codecs;
#[cfg(not(target_os = "android"))]
pub(crate) mod launcher;
pub(crate) mod library;
pub(crate) mod machine_facts;
pub(crate) mod media_assets;
pub(crate) mod media_players;
pub(crate) mod menus;
pub(crate) mod notifications;
pub(crate) mod opener;
pub(crate) mod phone_window;
pub(crate) mod program_apps;
#[cfg(target_os = "macos")]
pub(crate) mod prompt_dialogs;
pub(crate) mod questions;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod quitting;
pub(crate) mod recordings;
#[cfg(test)]
pub(crate) mod test_system;
#[cfg(test)]
mod tests;
#[cfg(not(target_os = "android"))]
pub(crate) mod waos_tray;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result};
use waft::{App, Entity, IntoWidget, run_widget};
use waft_cupertino::install_cupertino_icon_font;
use waft_single_instance::LaterLaunches;
use waft_winui::install_icon_font;
use waos::{
    EngineTime, FreezePolicy, HttpProxy, OpenRequest, RunningProgram, Shell, ShellCompositor,
    ShellFileDialogs, ShellRecordingIndicator, ShellVideoFrameReaders, VsyncWaiter, WaOS,
    WaOSConfig,
};
use waos_local::LocalCapabilities;
use waos_package::ProgramId;
use waos_program_window::WaftCompositor;
use waos_store::{SourceProxy, Store};

use crate::logic::asking_now::AskingNow;
use crate::logic::command_line::LibraryPlace;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::logic::dev_server::DevServer;
use crate::logic::environments::ProgramEnvironments;
use crate::logic::folders::AppFolders;
use crate::logic::frame_readers::ReadersOnRequest;
use crate::logic::installed_runs::InstalledRuns;
use crate::logic::machine_facts::platform_state;
use crate::logic::noted_file_dialogs::NotedFileDialogs;
use crate::logic::package_copies::PackageCopies;
use crate::logic::placement::WindowsOnRequest;
use crate::logic::preferences::PreferencesFile;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_identities::Identities;
use crate::logic::program_places::{PlacedWhereOpened, ProgramPlace, ProgramPlaces};
use crate::logic::program_shortcuts::SystemShortcuts;
use crate::logic::prompts::PromptsOnRequest;
use crate::logic::recording_indicator::RecordingIndicatorOnRequest;
use crate::logic::runs::Runs;
use crate::logic::vsync_waiter::WaftVsync;
use crate::ui::deck::Deck;
use crate::ui::every_window::{LibraryWindow, every_window, program_windows};
#[cfg(not(target_os = "android"))]
use crate::ui::launcher::launcher_window;
use cameras::Cameras;
use clipboard::Clipboard;
use deck::home::keep_library_with_the_home;
use deck::tabs::Tabs;
use file_dialogs::FileDialogs;
use given_views::follow_given_views;
use image_codecs::ImageCodecs;
#[cfg(not(target_os = "android"))]
use launcher::Launcher;
use library::{
    Library, ProgramOpened, ProgramsOpen, open_library, open_settings_when_asked,
    request_to_open_with_waos, take_files_opened_with_waos, take_notification_clicks,
};
use media_assets::MediaAssets;
use media_players::MediaPlayers;
use menus::Menus;
use notifications::Notifications;
use opener::Opener;
use phone_window::in_phone_window;
use questions::Questions;
use recordings::Recordings;
#[cfg(not(target_os = "android"))]
use waos_tray::WaosTray;

/// The windows WaOS shows in its own process for the programs running in it.
pub(crate) type ViewWindows = waos_program_window::ViewWindows<RunningProgram>;

/// Whether WaOS.app's bundle declares why WaOS uses the microphone, its
/// `NSMicrophoneUsageDescription`, without which macOS ends an app that opens it: it does, in
/// this crate's `[package.metadata.waft.usage_descriptions]`. So the programs' sound devices have
/// this computer's microphone, and a program's `open-microphone` asks its user; were the
/// declaration taken out, this goes false with it, and `open-microphone` answers `not-supported`
/// without asking.
const MICROPHONE_USAGE_DECLARED: bool = true;

/// Whether WaOS.app's bundle declares why WaOS uses the camera, its `NSCameraUsageDescription`,
/// without which macOS ends an app that opens it: it does, in this crate's
/// `[package.metadata.waft.usage_descriptions]`. So the shell has Waft's cameras, where Waft's
/// platform has them; were the declaration taken out, this goes false with it, and a program's
/// `get-cameras` answers none.
const CAMERA_USAGE_DECLARED: bool = true;

/// What WaOS's command line opened as it started WaOS.
#[cfg_attr(
    any(target_os = "android", target_os = "ios"),
    allow(dead_code, reason = "a phone gives WaOS no command line")
)]
pub(crate) enum OpenedAtStart {
    /// Files opened with WaOS itself, by their paths on the machine, as Windows and Linux start
    /// an app with the files it opens.
    WithWaos(Vec<PathBuf>),
    /// Files `waos open` asked to open with a program, no WaOS answering.
    WithProgram { id: ProgramId, request: OpenRequest },
    /// The program `waos open --program` asked for, as its pin does, with the activation token
    /// the desktop gave the launch.
    Program {
        id: ProgramId,
        activation_token: Option<String>,
    },
}

/// Starts the engine with the programs' apps as its placement on macOS, where each program has its
/// own, and as its menu bar, context menus, file dialogs and notifications there, and the desktop's
/// windows and Waft's notifications elsewhere, the questions shown over them as its prompts and its
/// picker of outputs, the library as its indicator of who records, WaOS.app declaring why it
/// records, Waft's vsync waiter as its screens' refreshes, Waft's compositor, where it has one, as
/// the one programs' layers are made on, Waft's clipboard as its clipboard and Waft's players,
/// reading of media, frame readers and cameras as its own, WaOS.app
/// declaring why it uses the camera, keeps the machine's facts current in it, opens the store,
/// starts the dev server, on a desktop, with this computer's capabilities and the store's global settings for
/// every program it runs, opens the library over the store with the packages opened with WaOS and
/// the clicks on the programs' notifications going to it, shows it in `place`, the programs'
/// windows and their questions, makes a quit ask every program first, and opens what `opened`
/// names, with WaOS or with their program. What `later_launches` of WaOS pass on is read by WaOS's
/// command line and opened so, as the system's own requests are.
pub(crate) fn start(
    app: &mut App,
    folders: AppFolders,
    place: LibraryPlace,
    opened: Option<OpenedAtStart>,
    later_launches: Option<LaterLaunches>,
) -> Result<()> {
    let facts = platform_state(app.platform().as_ref());
    let programs_open = programs_open(place);
    let preferences = own_preferences(place, &folders);
    let store = Store::open(&folders.store, SourceProxy::System)
        .with_context(|| format!("opening the store at {}", folders.store.display()))?;
    let package_copies =
        PackageCopies::clear(folders.package_copies.clone()).with_context(|| {
            format!(
                "clearing the packages' copies at {}",
                folders.package_copies.display()
            )
        })?;
    let faces = program_faces(&store, &folders);
    let (own_windows, requests) = WindowsOnRequest::new(faces.clone());
    let program_apps = program_apps::start(&folders, &store);
    let places = ProgramPlaces::new(usual_place(place));
    let placement = Arc::new(PlacedWhereOpened {
        places: places.clone(),
        windows: program_apps::placement(&program_apps, own_windows),
    });
    let asking = AskingNow::new();
    let (prompts, prompted) = PromptsOnRequest::new(Arc::clone(&asking));
    let output_picker = prompts.output_picker();
    let machine_picker = prompts.machine_picker();
    let (recording_indicator, recording_changes) = RecordingIndicatorOnRequest::new();
    let waiter = app
        .platform()
        .vsync_waiter()
        .context("Waft's platform has no screen refreshes to pace programs by")?;
    let vsync = Arc::new(WaftVsync::new(waiter));
    let compositor = app
        .platform()
        .compositor()
        .map(|compositor| Arc::new(WaftCompositor::new(compositor)) as Arc<dyn ShellCompositor>);
    let clipboard = Clipboard::of(app.platform().as_ref());
    let menus = Menus::of(app.platform().as_ref());
    let file_dialogs = FileDialogs::of(app.platform().as_ref());
    let opener = Opener::of(app.platform().as_ref());
    let notifications = Notifications::of(app.platform().as_ref());
    let media_players = MediaPlayers::of(app.platform().as_ref());
    let media_assets = MediaAssets::of(app.platform().as_ref());
    let image_codecs = ImageCodecs::of(app.platform().as_ref());
    let cameras = CAMERA_USAGE_DECLARED
        .then(|| Cameras::of(app.platform().as_ref()))
        .flatten();
    let shell = Shell {
        views: placement,
        vsync: Arc::clone(&vsync) as Arc<dyn VsyncWaiter>,
        compositor,
        prompts: Arc::new(prompts),
        clipboard: clipboard.as_ref().map(Clipboard::shell),
        menu_bar: program_apps::menu_bar(&program_apps),
        context_menus: program_apps::context_menus(&program_apps, menus.context_menus()),
        tray_icons: menus.tray_icons(),
        audio_output_picker: Some(Arc::new(output_picker)),
        // Wherever a program may record, with either device, which each declaration decides.
        recording_indicator: (MICROPHONE_USAGE_DECLARED || CAMERA_USAGE_DECLARED)
            .then(|| Arc::new(recording_indicator) as Arc<dyn ShellRecordingIndicator>),
        file_dialogs: program_apps::file_dialogs(
            &program_apps,
            file_dialogs.as_ref().map(FileDialogs::shell),
        )
        .map(|dialogs| {
            let asking = Arc::clone(&asking);
            Arc::new(NotedFileDialogs { dialogs, asking }) as Arc<dyn ShellFileDialogs>
        }),
        opener: opener.as_ref().map(Opener::shell),
        notifications: program_apps::notifications(
            &program_apps,
            notifications.as_ref().map(Notifications::shell),
        ),
        media_players: media_players.as_ref().map(MediaPlayers::shell),
        media_assets: media_assets.as_ref().map(MediaAssets::shell),
        image_codecs: image_codecs.as_ref().map(ImageCodecs::shell),
        video_frame_readers: app
            .platform()
            .video_frame_readers()
            .map(|_| Arc::new(ReadersOnRequest) as Arc<dyn ShellVideoFrameReaders>),
        cameras: cameras.as_ref().map(Cameras::shell),
        simulated_cameras: simulated_cameras(image_codecs.as_ref().map(ImageCodecs::shell)),
        // A machine is picked on a sheet put as a program's questions are.
        machine_picker: Some(Arc::new(machine_picker)),
    };
    let config = WaOSConfig {
        code_cache: folders.code_cache,
        freeze: FreezePolicy::default(),
        native_unwind_info: native_unwind_info_asked(),
        http_proxy: HttpProxy::System,
        time: EngineTime::Real,
        temporary_folders: folders.temporary_folders,
    };
    if config.native_unwind_info {
        tracing::info!("programs are compiled with native unwind information");
    }
    let engine = Arc::new(WaOS::new(config, shell, facts).context("starting the engine")?);
    vsync.attach(&engine);
    machine_facts::keep_current(app, Arc::clone(&engine));
    if let Some(clipboard) = clipboard {
        clipboard.serve(app, Arc::clone(&engine));
    }
    let view_windows = ViewWindows::listen(app, requests);
    let posted_as_programs =
        notifications.map(|notifications| notifications.serve(app, store.clone()));
    let capabilities = if MICROPHONE_USAGE_DECLARED {
        LocalCapabilities::new()
    } else {
        LocalCapabilities::without_microphone()
    };
    let environments = ProgramEnvironments::new(capabilities, store.clone());
    let runs = Arc::new(Runs::default());
    // What a tab closed on a phone stops its program among.
    let tabs_runs = Arc::clone(&runs);
    let installed_runs = InstalledRuns::new(Arc::clone(&engine), environments, runs, places);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    listen_for_cargo_waos(
        app,
        installed_runs.clone(),
        &store,
        asking,
        folders.dev_components,
    )?;
    let prompt_windows = questions::prompt_windows(app, view_windows.clone(), faces);
    let questions_store = store.clone();
    let recordings = Recordings::listen(app, recording_changes, store.clone());
    let library = open_library(
        app,
        store,
        installed_runs.clone(),
        package_copies,
        programs_open,
        preferences,
    )?;
    bring_opened_programs_forward(app, &library, &view_windows);
    let prompts = program_apps::follow(
        app,
        program_apps,
        installed_runs,
        &library,
        Rc::clone(&prompt_windows),
    );
    Questions::listen(app, prompted.questions, prompts.placement, questions_store);
    match opened {
        Some(OpenedAtStart::WithProgram { id, request }) => {
            library.update(app, |library, cx| library.open_with(&id, request, cx));
        }
        Some(OpenedAtStart::Program {
            id,
            activation_token,
        }) => {
            library.update(app, |library, cx| {
                library.show_program(&id, activation_token, cx)
            });
        }
        // Waits for the library to take the files opened with WaOS, below.
        Some(OpenedAtStart::WithWaos(files)) => {
            app.open_requested(request_to_open_with_waos(files));
        }
        None => {}
    }
    open_settings_when_asked(app, &library, prompted.settings);
    let library_window = library_window(
        app,
        place,
        library.clone(),
        recordings,
        tabs_runs,
        &view_windows,
    );
    // Once the panel follows the library: a click on a program's notification that started WaOS
    // closes the panel it opened at launch, as opening a program does, and a package opens it.
    take_files_opened_with_waos(app, &library, later_launches);
    take_notification_clicks(app, &library, posted_as_programs);
    // Once the library opens programs in the deck's tabs, where a shortcut's tap may open one.
    keep_program_shortcuts(app, &library);
    let program_windows = program_windows(&view_windows, &prompt_windows);
    let windows = every_window(
        Some(library_window),
        program_windows,
        prompt_windows,
        prompts.views,
    );
    run_widget(app, windows);
    // After `run_widget`, which installs the font collection the icons register into.
    install_icon_font(app);
    install_cupertino_icon_font(app);
    if let Some(file_dialogs) = file_dialogs {
        file_dialogs.serve(app, view_windows.clone());
    }
    if let Some(opener) = opener {
        opener.serve(app);
    }
    if let Some(media_players) = media_players {
        media_players.serve(app);
    }
    if let Some(media_assets) = media_assets {
        media_assets.serve(app);
    }
    if let Some(image_codecs) = image_codecs {
        image_codecs.serve(app);
    }
    if let Some(cameras) = cameras {
        cameras.serve(app);
    }
    menus.serve(app, view_windows);
    Ok(())
}

/// Listens for `cargo waos` on the socket `waos-dev-protocol` names, running what it sends on
/// `installed`'s engine with its environments, counted in its runs, the components kept in
/// `dev_components`, and opening files with an installed program as `installed` opens it; and
/// makes a quit ask every program in its runs first, stopping the server once they agree. Not on
/// a phone: `cargo waos` reaches none yet, and the system ends an app there without asking it.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn listen_for_cargo_waos(
    app: &mut App,
    installed: InstalledRuns,
    store: &Store,
    asking: Arc<AskingNow>,
    dev_components: PathBuf,
) -> Result<()> {
    let socket = waos_dev_protocol::listener_path();
    let runs = Arc::clone(&installed.runs);
    let server = DevServer::start(installed, store.clone(), asking, &socket, dev_components)
        .with_context(|| format!("listening for cargo waos at {}", socket.display()))?;
    tracing::info!(socket = %socket.display(), "WaOS listens for cargo waos");
    quitting::ask_programs_before_quitting(app, runs, server);
    Ok(())
}

/// Whether this system has WaOS's simulated camera: macOS and Android, where the engine takes a
/// camera's frames; elsewhere a program answered simulated lists no camera, and its settings page
/// says so.
pub(crate) const SIMULATED_CAMERA: bool = cfg!(any(target_os = "macos", target_os = "android"));

/// WaOS's simulated cameras, which encode their photos with `codecs`, where this system has them.
#[cfg(any(target_os = "macos", target_os = "android"))]
pub(crate) fn simulated_cameras(
    codecs: Option<Arc<dyn waos::ShellImageCodecs>>,
) -> Option<Arc<dyn waos::ShellCameras>> {
    use crate::logic::simulated_cameras::SimulatedCameras;
    Some(Arc::new(SimulatedCameras::new(codecs)))
}

/// None: the simulated camera is not built on this system (`SIMULATED_CAMERA`).
#[cfg(not(any(target_os = "macos", target_os = "android")))]
pub(crate) fn simulated_cameras(
    _codecs: Option<Arc<dyn waos::ShellImageCodecs>>,
) -> Option<Arc<dyn waos::ShellCameras>> {
    None
}

/// Where a program goes in WaOS shown in `place` unless it was opened somewhere else: in windows,
/// a desktop's or on Android a task each, but in the deck's tabs in a phone's place on a desktop,
/// where nothing else shows a program.
fn usual_place(place: LibraryPlace) -> ProgramPlace {
    if place == LibraryPlace::Phone && !cfg!(target_os = "android") {
        ProgramPlace::Tab
    } else {
        ProgramPlace::Windows
    }
}

/// Where the programs the library opens open in WaOS shown in `place`: in windows on a desktop, in
/// the deck's tabs in a phone's place there, and on Android where the user chose.
fn programs_open(place: LibraryPlace) -> ProgramsOpen {
    if cfg!(target_os = "android") {
        return ProgramsOpen::AsChosen;
    }
    match place {
        LibraryPlace::Launcher | LibraryPlace::ClosedLauncher => ProgramsOpen::InWindows,
        LibraryPlace::Phone => ProgramsOpen::InTabs,
    }
}

/// The file of WaOS's own preferences among `folders`, where WaOS shown in `place` offers a
/// choice of its own: on Android, where programs open, and in a desktop's launcher, whether WaOS
/// is kept in the Dock or the taskbar; none in a phone's window on a desktop.
fn own_preferences(place: LibraryPlace, folders: &AppFolders) -> Option<PreferencesFile> {
    let offers_a_choice = cfg!(target_os = "android") || place != LibraryPlace::Phone;
    offers_a_choice.then(|| PreferencesFile::at(folders.preferences.clone()))
}

/// A program opened from `library` that has windows of its own among `windows` brings its front
/// one forward, as its task comes forward on Android, with the activation token of the launch that
/// opened it, which a program with no window yet shows its next one with.
pub(crate) fn bring_opened_programs_forward(
    app: &mut App,
    library: &Entity<Library>,
    windows: &Entity<ViewWindows>,
) {
    let windows = windows.downgrade();
    app.subscribe(library, move |app, opened: &ProgramOpened| {
        if let Some(windows) = windows.upgrade() {
            let (id, token) = (opened.id.as_str(), opened.activation_token.clone());
            windows.update(app, |windows, cx| {
                windows.bring_forward_for_launch(id, token, cx);
            });
        }
    })
    .detach();
}

/// The installed programs' shortcuts kept where the system keeps them, through `library`, on
/// Android, and each tap on one that opens inside WaOS opening its program there, or bringing it
/// forward where it runs.
fn keep_program_shortcuts(app: &mut App, library: &Entity<Library>) {
    let Some(system) = system_shortcuts(app) else {
        return;
    };
    let (taps_to, cx) = (library.downgrade(), app.to_async());
    system.hear_taps(Box::new(move |id| {
        let library = taps_to.clone();
        cx.post(move |app| {
            if let Some(library) = library.upgrade() {
                library.update(app, |library, cx| library.show_program(&id, None, cx));
            }
        });
    }));
    library.update(app, |library, cx| library.keep_shortcuts(system, cx));
}

/// Android's shortcuts, through Waft's Android host.
#[cfg(target_os = "android")]
fn system_shortcuts(app: &App) -> Option<Rc<dyn SystemShortcuts>> {
    crate::os::system_shortcuts(app.platform().as_ref())
}

/// None: only Android keeps an app's shortcuts of WaOS's programs.
#[cfg(not(target_os = "android"))]
fn system_shortcuts(_app: &App) -> Option<Rc<dyn SystemShortcuts>> {
    None
}

/// WaOS's own window, the library shown in it with the programs recording, in `place`: the
/// launcher's, shown from launch or closed, which hands the focus back to the programs' windows
/// among `view_windows`, or a phone's, with the deck's tabs, whose closing stops a program among `runs`,
/// and the programs' windows the platform gives WaOS kept among `view_windows`.
fn library_window(
    app: &mut App,
    place: LibraryPlace,
    library: Entity<Library>,
    recordings: Entity<Recordings>,
    runs: Arc<Runs>,
    view_windows: &Entity<ViewWindows>,
) -> LibraryWindow {
    match place {
        #[cfg(not(target_os = "android"))]
        LibraryPlace::Launcher | LibraryPlace::ClosedLauncher => {
            let launcher = Launcher::new(app, &library, view_windows.clone());
            launcher.update(app, |launcher, cx| {
                if place == LibraryPlace::Launcher {
                    launcher.summoned("WaOS started", cx);
                } else {
                    launcher.started_closed(cx);
                }
            });
            WaosTray::follow(app, &library, &launcher);
            launcher_window(&launcher, &library, &recordings)
        }
        // Android has no desktop, whose launcher WaOS is there: the library is a phone's.
        #[cfg(target_os = "android")]
        LibraryPlace::Launcher | LibraryPlace::ClosedLauncher => {
            let tabs = Tabs::listen(app, runs);
            phone_deck(app, library, recordings, tabs, view_windows)
        }
        LibraryPlace::Phone => {
            let tabs = Tabs::listen(app, runs);
            phone_deck(app, library, recordings, tabs, view_windows)
        }
    }
}

/// The deck, its tabs the home, which is the library, and each program opened in it, laid out as
/// on a phone: in WaOS's own view the platform gives, as a phone gives its app the screen, or,
/// where it gives none at the start, in a window of a phone's size it asks for, as Waft's
/// `run_app_with` does for an app's own view (`phone_window`); its tabs are `tabs`. A program's
/// window brought back from Recents, which no running program asked for, is `given_views`': a
/// first one starts its program again in `view_windows`, an extra one closing.
fn phone_deck(
    app: &mut App,
    library: Entity<Library>,
    recordings: Entity<Recordings>,
    tabs: Entity<Tabs>,
    view_windows: &Entity<ViewWindows>,
) -> LibraryWindow {
    library.update(app, |library, _| library.open_in(tabs.clone()));
    keep_library_with_the_home(app, &library, &tabs);
    let deck = Deck {
        tabs: tabs.clone(),
        library: library.clone(),
        recordings,
    };
    let window = in_phone_window(app, deck.into_widget());
    follow_given_views(app, &library, &tabs, view_windows);
    window
}

/// The faces of the programs in `store`, with the identities of the installed ones where a window
/// is an app of its own, their icons for the system among `folders`.
fn program_faces(store: &Store, folders: &AppFolders) -> ProgramFaces {
    ProgramFaces::new(store.clone(), identities(folders))
}

/// What the programs' identities are made of, their icons among `folders`; none on a system whose
/// windows are no apps of their own.
#[cfg(not(target_os = "android"))]
fn identities(folders: &AppFolders) -> Option<Identities> {
    Identities::of_this_waos(folders.program_icons.clone())
}

#[cfg(target_os = "android")]
fn identities(_folders: &AppFolders) -> Option<Identities> {
    None
}

/// Whether WaOS was started with `WAOS_NATIVE_UNWIND_INFO=1` in its environment, asking for
/// programs compiled with what a native profiler or debugger needs to walk through their frames
/// (`WaOSConfig::native_unwind_info`).
fn native_unwind_info_asked() -> bool {
    std::env::var_os("WAOS_NATIVE_UNWIND_INFO").is_some_and(|value| value == "1")
}
