//! What ties the app to Waft's application: the engine started with what Waft reports of the
//! machine and kept current with it, each view a program asks for in a window of its own on a
//! desktop, and on a phone each program in a tab of the deck, which refuses every request, Waft's
//! clipboard as the programs' one, Waft's menus as theirs, a program's menu bar, context menus,
//! file dialogs and notifications its app's on macOS, WaOS's own menu bar, Waft's opener,
//! notifications elsewhere, players, reading of media and cameras as theirs, the dev server listening for `waos`, the installed programs' shortcuts on Android, the library over the
//! store in the desktop's launcher or in a window of a phone's size, the questions
//! programs put to their user, the picker of their sound's output among them, shown on macOS in
//! dialogs of the programs' apps (`prompt_dialogs`), the programs recording, and the quit that
//! asks every program first; and on macOS the activity that keeps App Nap from throttling WaOS
//! while WaOS installs or optimizes a program or a program's window is in sight
//! (`app_nap_exemption`).
//!
//! The invariant: nothing here is touched off the main thread, and the system is asked on it alone;
//! what the programs' commands read of it, This computer's home, is kept for them
//! (`logic::phone_files`). What reaches it from the programs'
//! threads and the dev server's comes through a queue a task of the main thread's empties: the view
//! requests (`logic::placement`), the questions (`logic::prompts`), who records
//! (`logic::recording_indicator`), the clipboard's calls (`logic::clipboard_calls`), the menus'
//! calls (`logic::menu_calls`), the notifications' calls
//! (`logic::notification_calls`), each view's calls (`waos-program-window`'s), on macOS what
//! the programs' apps send (`logic::program_apps`), the presence of each of their windows among
//! it, what the package manager is doing (`waos_services::packages`), and the calls posted to a value
//! on the main thread that serves them (`app_thread_bound`): the players' (`media_players`), the media
//! assets' (`media_assets`), the image codecs' (`image_codecs`), the cameras' (`cameras`) and the openings (`opener`).

#[cfg(any(target_os = "macos", test))]
pub(crate) mod app_nap_exemption;
mod app_suspension;
mod app_thread_bound;
pub(crate) mod cameras;
pub(crate) mod clipboard;
mod days_kept;
pub(crate) mod deck;
#[cfg(desktop)]
pub(crate) mod desktop_host;
pub(crate) mod file_dialogs;
pub(crate) mod given_views;
pub(crate) mod image_codecs;
#[cfg(desktop)]
pub(crate) mod launcher;
pub(crate) mod library;
pub(crate) mod machine_facts;
pub(crate) mod media_assets;
pub(crate) mod media_players;
pub(crate) mod menus;
pub(crate) mod notifications;
pub(crate) mod opener;
pub(crate) mod phone_files;
pub(crate) mod phone_window;
pub(crate) mod program_apps;
#[cfg(target_os = "macos")]
pub(crate) mod prompt_dialogs;
pub(crate) mod questions;
#[cfg(desktop)]
pub(crate) mod quitting;
pub(crate) mod recordings;
#[cfg(test)]
pub(crate) mod test_system;
#[cfg(test)]
pub(crate) mod test_turns;
#[cfg(test)]
mod tests;
pub(crate) mod video_decoders;
#[cfg(desktop)]
pub(crate) mod waos_tray;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::channel::mpsc::UnboundedReceiver;
use waft::{App, Entity, IntoWidget, run_widget};
use waft_cupertino::install_cupertino_icon_font;
use waft_single_instance::LaterLaunches;
use waft_winui::install_icon_font;
use waos::{
    Compiler, EngineTime, HttpProxy, OpenRequest, RunningProgram, Shell, ShellCompositor,
    ShellFileDialogs, ShellRecordingIndicator, ShellSystemPermissions, ShellVideoFrameReaders,
    VsyncWaiter,
};
use waos_local::LocalCapabilities;
use waos_package::ProgramId;
use waos_program_window::WaftCompositor;
use waos_services::packages::PackageManager;
use waos_services::permissions::Prompts;
use waos_services::sources::SourceProxy;

use crate::logic::asking_now::AskingNow;
use crate::logic::command_line::LibraryPlace;
#[cfg(desktop)]
#[cfg(desktop)]
use crate::logic::dev_server::{DevServer, ServedDomains};
#[cfg(desktop)]
use crate::logic::developer_access::DeveloperAccess;
use crate::logic::folders::AppFolders;
use crate::logic::frame_readers::ReadersOnRequest;
use crate::logic::machine_facts::platform_state;
use crate::logic::noted_file_dialogs::NotedFileDialogs;
use crate::logic::package_copies::PackageCopies;
use crate::logic::phone_files::{PhoneFilesAccess, ThisComputerHome};
use crate::logic::placement::{ProgramWindowRequest, WindowsOnRequest};
use crate::logic::preferences::PreferencesFile;
use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_identities::Identities;
use crate::logic::program_places::{PlacedWhereOpened, ProgramPlace, TabRequest, TabsOnRequest};
use crate::logic::program_shortcuts::SystemShortcuts;
use crate::logic::prompts::{PromptRequests, PromptsOnRequest};
use crate::logic::recording_indicator::RecordingIndicatorOnRequest;
use crate::logic::running_apps::RunningApps;
use crate::logic::startup::{self, BeforeTheEngine, EngineCompiler, Outside, Setup};
use crate::logic::vsync_waiter::WaftVsync;
use crate::ui::deck::Deck;
use crate::ui::every_window::{LibraryWindow, every_window, program_windows};
#[cfg(desktop)]
use crate::ui::launcher::launcher_window;
use cameras::Cameras;
use clipboard::Clipboard;
use deck::home::keep_library_with_the_home;
use deck::tabs::Tabs;
use file_dialogs::FileDialogs;
use given_views::follow_given_views;
use image_codecs::ImageCodecs;
#[cfg(desktop)]
use launcher::Launcher;
#[cfg(desktop)]
use library::offer_developer_access;
use library::{
    FirstWindowWanted, Library, LibraryDomains, ProgramOpened, ProgramsOpen, open_library,
    open_settings_when_asked, request_to_open_with_waos, show_phone_files,
    take_files_opened_with_waos, take_notification_clicks,
};
use media_assets::MediaAssets;
use media_players::MediaPlayers;
use menus::Menus;
use notifications::Notifications;
use opener::Opener;
use phone_files::{PhoneFiles, follow_phone_files, phone_files_on_request};
use phone_window::in_phone_window;
use questions::Questions;
use recordings::Recordings;
use video_decoders::VideoDecoders;
#[cfg(desktop)]
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
    mobile,
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
/// command line and opened so, as the system's own requests are. Where WaOS reaches the phone's
/// files only as the user allows, `phone_files` says whether it may, read on the main thread as
/// WaOS starts and comes back to the front, and This computer's home follows it; a pick of This
/// computer asks for it, and the global settings say it.
pub(crate) fn start(
    app: &mut App,
    folders: AppFolders,
    place: LibraryPlace,
    opened: Option<OpenedAtStart>,
    later_launches: Option<LaterLaunches>,
    phone_files: Option<Rc<dyn PhoneFilesAccess>>,
) -> Result<()> {
    let programs_open = programs_open(place);
    let preferences = own_preferences(place, &folders);
    let package_copies =
        PackageCopies::clear(folders.package_copies.clone()).with_context(|| {
            format!(
                "clearing the packages' copies at {}",
                folders.package_copies.display()
            )
        })?;
    let work = library::work()?;
    // The activity macOS must not throttle WaOS under, from now until WaOS ends: the programs
    // that compile and the windows in sight tell it (`state::app_nap_exemption`).
    #[cfg(target_os = "macos")]
    let app_nap_exemption = app_nap_exemption::start(app);
    let waft = WaftParts::of(app);
    let (recording_indicator, recording_changes) = RecordingIndicatorOnRequest::new();
    let setup = desktop_setup(&folders, place, work.handle().clone())?;
    let (started, made) = startup::start(
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
            let asked = Asked { prompts, prompted };
            desktop_outside(
                app,
                &folders,
                &waft,
                recording_indicator,
                phone_files,
                before,
                asked,
            )
        },
    )?;
    let startup::Started {
        running,
        packages,
        permissions,
        sources,
        engine,
        environments,
        places,
        #[cfg(desktop)]
        outputs,
        starter,
        ..
    } = started;
    let MadeWithTheShell {
        requests,
        faces,
        program_apps,
        tab_requests,
        #[cfg(desktop)]
        asking,
        prompted,
        phone_files,
        phone_storage,
        ..
    } = made;
    let WaftParts {
        clipboard,
        menus,
        file_dialogs,
        opener,
        notifications,
        media_players,
        media_assets,
        image_codecs,
        video_decoders,
        cameras,
    } = waft;
    if let Err(error) = sources.start_with(&library::first_sources()) {
        tracing::warn!("the sources WaOS comes with are not added: {error}");
    }
    machine_facts::keep_current(app, Arc::clone(&engine));
    days_kept::keep_every_day_when_paused(app, Arc::clone(&engine));
    app_suspension::follow_app_suspension(app, Arc::clone(&engine));
    if let Some(clipboard) = clipboard {
        clipboard.serve(app, Arc::clone(&engine));
    }
    let view_windows = ViewWindows::listen(app, requests);
    let posted_as_programs =
        notifications.map(|notifications| notifications.serve(app, Arc::clone(&packages)));
    // What a tab closed on a phone stops its program among.
    let tabs_runs = Arc::clone(&running);
    #[cfg(target_os = "macos")]
    app_nap_exemption::follow_sessions(app, app_nap_exemption.clone(), packages.sessions());
    #[cfg(target_os = "macos")]
    app_nap_exemption::follow_starts(app, app_nap_exemption.clone(), starter.starting());
    #[cfg(desktop)]
    let developer_access = DeveloperAccess::at(folders.developer_access.clone());
    #[cfg(desktop)]
    let served = ServedDomains {
        running: Arc::clone(&running),
        packages: Arc::clone(&packages),
        starter: Arc::clone(&starter),
        permissions: permissions.clone(),
        environments: Arc::clone(&environments),
        outputs,
        engine: Arc::clone(&engine),
    };
    #[cfg(desktop)]
    listen_for_the_command(app, served, asking, developer_access.clone())?;
    let prompt_windows = questions::prompt_windows(app, view_windows.clone(), faces);
    let recordings = Recordings::listen(app, recording_changes, Arc::clone(&packages));
    let domains = LibraryDomains {
        sources,
        packages: Arc::clone(&packages),
        running: Arc::clone(&running),
        starter: Arc::clone(&starter),
        permissions,
        environments,
        places,
        engine,
    };
    let library = open_library(
        app,
        domains,
        package_copies,
        programs_open,
        preferences,
        work,
    );
    #[cfg(desktop)]
    offer_developer_access(app, &library, developer_access);
    bring_opened_programs_forward(app, &library, &view_windows);
    #[cfg(target_os = "macos")]
    let prompts = program_apps::follow(
        app,
        program_apps,
        &running,
        starter,
        &library,
        Rc::clone(&prompt_windows),
        &app_nap_exemption,
    );
    #[cfg(not(target_os = "macos"))]
    let prompts = program_apps::follow(program_apps, Rc::clone(&prompt_windows));
    Questions::listen(app, prompted.questions, prompts.placement, packages);
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
    if let Some(phone_files) = phone_files.clone() {
        show_phone_files(app, &library, phone_files);
    }
    let library_window = library_window(
        app,
        place,
        library.clone(),
        recordings,
        tabs_runs,
        tab_requests,
        &view_windows,
    );
    // Once the panel follows the library: a click on a program's notification that started WaOS
    // closes the panel it opened at launch, as opening a program does, and a package opens it.
    take_files_opened_with_waos(app, &library, later_launches);
    take_notification_clicks(app, &library, posted_as_programs);
    // Once the library opens programs in the deck's tabs, where a shortcut's tap may open one.
    keep_program_shortcuts(app, &library);
    let program_windows = program_windows(&view_windows, &prompt_windows);
    let prompt_windows_for_dialogs = Rc::clone(&prompt_windows);
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
        // A phone shows a dialog only over a screen; a desktop's floats where its program has none.
        let screens_of_their_own = cfg!(mobile).then(|| Rc::clone(&prompt_windows_for_dialogs));
        file_dialogs.serve(
            app,
            view_windows.clone(),
            screens_of_their_own,
            phone_storage,
        );
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
    if let Some(video_decoders) = video_decoders {
        video_decoders.serve(app);
    }
    if let Some(cameras) = cameras {
        cameras.serve(app);
    }
    menus.serve(app, view_windows);
    Ok(())
}

/// How the WaOS app starts: its store and its apps' `/tmp` among `folders`, compiling with native
/// unwind information where it was asked for, through the system's proxy, freezing an app out of
/// sight as the system does, its apps' views going where WaOS shown in `place` puts them, and an
/// inspector's starts on `work`, the library's runtime.
fn desktop_setup(
    folders: &AppFolders,
    place: LibraryPlace,
    work: tokio::runtime::Handle,
) -> Result<Setup> {
    let native_unwind_info = native_unwind_info_asked();
    if native_unwind_info {
        tracing::info!("programs are compiled with native unwind information");
    }
    let compiler = Compiler::new(native_unwind_info).context("starting the compiler")?;
    Ok(Setup {
        store: folders.store.clone(),
        temporary_folders: folders.temporary_folders.clone(),
        install_compiler: Arc::new(EngineCompiler(compiler.clone())),
        compiler,
        source_proxy: SourceProxy::System,
        http_proxy: HttpProxy::System,
        freeze: crate::os::freeze_policy(),
        time: EngineTime::Real,
        usual_place: usual_place(place),
        work,
    })
}

/// What asks the programs' users, and where its questions arrive on the main thread.
struct Asked {
    prompts: Arc<PromptsOnRequest>,
    prompted: PromptRequests,
}

/// What Waft's platform gives the engine's shell, made before the engine and served once it is.
struct WaftParts {
    clipboard: Option<Clipboard>,
    menus: Menus,
    file_dialogs: Option<FileDialogs>,
    opener: Option<Opener>,
    notifications: Option<Notifications>,
    media_players: Option<MediaPlayers>,
    media_assets: Option<MediaAssets>,
    image_codecs: Option<ImageCodecs>,
    video_decoders: Option<VideoDecoders>,
    cameras: Option<Cameras>,
}

impl WaftParts {
    /// What `app`'s platform has, its cameras where WaOS.app declares why it uses them.
    fn of(app: &App) -> WaftParts {
        let platform = app.platform();
        let platform = platform.as_ref();
        WaftParts {
            clipboard: Clipboard::of(platform),
            menus: Menus::of(platform),
            file_dialogs: FileDialogs::of(platform),
            opener: Opener::of(platform),
            notifications: Notifications::of(platform),
            media_players: MediaPlayers::of(platform),
            media_assets: MediaAssets::of(platform),
            image_codecs: ImageCodecs::of(platform),
            video_decoders: VideoDecoders::of(platform),
            cameras: CAMERA_USAGE_DECLARED
                .then(|| Cameras::of(platform))
                .flatten(),
        }
    }
}

/// What the desktop app made with the engine's shell and keeps: where the programs' windows are
/// asked for and how they are shown, their apps, the tabs asked for, what their users are asked
/// now and the questions, and the phone's files, where WaOS reaches them only as the user allows.
struct MadeWithTheShell {
    requests: UnboundedReceiver<ProgramWindowRequest>,
    faces: ProgramFaces,
    program_apps: program_apps::Started,
    tab_requests: UnboundedReceiver<TabRequest>,
    /// What the programs' users are asked now, which the dev server tells; a phone has none.
    #[cfg_attr(mobile, expect(dead_code, reason = "the dev server's alone"))]
    asking: Arc<AskingNow>,
    prompted: PromptRequests,
    phone_files: Option<Entity<PhoneFiles>>,
    phone_storage: Option<PathBuf>,
}

/// The engine's shell in the WaOS app, made from the domains `before` the engine: the programs'
/// apps as its placement on macOS, where each program has its own, and as its menu bar, context
/// menus, file dialogs and notifications there, and the desktop's windows and `waft`'s parts
/// elsewhere, placed where `before`'s places say; the questions `asked` puts as its pickers of
/// outputs and machines; `indicator` as its indicator of who records, WaOS.app declaring why it
/// records; Waft's vsync waiter as its screens' refreshes and Waft's compositor, where it has one,
/// as the one programs' layers are made on; and this computer's capabilities, its home the one
/// the phone's files keep, where `phone_files` says WaOS reaches them only as the user allows.
fn desktop_outside(
    app: &mut App,
    folders: &AppFolders,
    waft: &WaftParts,
    indicator: RecordingIndicatorOnRequest,
    phone_files: Option<Rc<dyn PhoneFilesAccess>>,
    before: BeforeTheEngine<'_>,
    asked: Asked,
) -> Result<Outside<MadeWithTheShell>> {
    let Asked { prompts, prompted } = asked;
    let facts = platform_state(app.platform().as_ref());
    let faces = program_faces(before.packages, folders);
    let (own_windows, requests) = WindowsOnRequest::new(faces.clone());
    let program_apps = program_apps::start(folders, before.packages);
    let (tabs_on_request, tab_requests) = TabsOnRequest::new();
    let placement = Arc::new(PlacedWhereOpened {
        places: before.places.clone(),
        tabs: tabs_on_request,
        windows: program_apps::placement(&program_apps, own_windows),
    });
    let asking = AskingNow::new(before.permissions.asking());
    let output_picker = prompts.output_picker(Arc::clone(&asking));
    let (phone_files_on_request, phone_files_followed) = phone_files
        .as_ref()
        .map(|access| phone_files_on_request(access.as_ref(), prompts.phone_files_off()))
        .unzip();
    let machine_picker =
        prompts.machine_picker(Arc::clone(&asking), phone_files_on_request.clone());
    let waiter = app
        .platform()
        .vsync_waiter()
        .context("Waft's platform has no screen refreshes to pace programs by")?;
    let vsync = Arc::new(WaftVsync::new(waiter));
    let compositor = app
        .platform()
        .compositor()
        .map(|compositor| Arc::new(WaftCompositor::new(compositor)) as Arc<dyn ShellCompositor>);
    let shell = Shell {
        views: placement,
        vsync: Arc::clone(&vsync) as Arc<dyn VsyncWaiter>,
        compositor,
        system_permissions: phone_files_on_request
            .map(|phone_files| Arc::new(phone_files) as Arc<dyn ShellSystemPermissions>)
            .or_else(|| program_apps::system_permissions(&program_apps)),
        clipboard: waft.clipboard.as_ref().map(Clipboard::shell),
        menu_bar: program_apps::menu_bar(&program_apps),
        context_menus: program_apps::context_menus(&program_apps, waft.menus.context_menus()),
        tray_icons: waft.menus.tray_icons(),
        audio_output_picker: Some(Arc::new(output_picker)),
        // Wherever a program may record, with either device, which each declaration decides.
        recording_indicator: (MICROPHONE_USAGE_DECLARED || CAMERA_USAGE_DECLARED)
            .then(|| Arc::new(indicator) as Arc<dyn ShellRecordingIndicator>),
        file_dialogs: program_apps::file_dialogs(
            &program_apps,
            waft.file_dialogs.as_ref().map(FileDialogs::shell),
        )
        .map(|dialogs| {
            let asking = Arc::clone(&asking);
            Arc::new(NotedFileDialogs { dialogs, asking }) as Arc<dyn ShellFileDialogs>
        }),
        opener: waft.opener.as_ref().map(Opener::shell),
        notifications: program_apps::notifications(
            &program_apps,
            waft.notifications.as_ref().map(Notifications::shell),
        ),
        media_players: waft.media_players.as_ref().map(MediaPlayers::shell),
        media_assets: waft.media_assets.as_ref().map(MediaAssets::shell),
        image_codecs: waft.image_codecs.as_ref().map(ImageCodecs::shell),
        video_frame_readers: app
            .platform()
            .video_frame_readers()
            .map(|_| Arc::new(ReadersOnRequest) as Arc<dyn ShellVideoFrameReaders>),
        gpus: waft.video_decoders.as_ref().map(VideoDecoders::shell),
        cameras: waft.cameras.as_ref().map(Cameras::shell),
        simulated_cameras: simulated_cameras(waft.image_codecs.as_ref().map(ImageCodecs::shell)),
        // A machine is picked on a sheet put as a program's questions are.
        machine_picker: Some(Arc::new(machine_picker)),
    };
    let phone_storage = phone_files_followed
        .as_ref()
        .map(|followed| followed.storage.root().to_owned());
    let phone_files = phone_files
        .zip(phone_files_followed)
        .map(|(access, followed)| follow_phone_files(app, access, followed));
    let home = phone_files
        .as_ref()
        .map(|phone_files| phone_files.read(app).home());
    let made = MadeWithTheShell {
        requests,
        faces,
        program_apps,
        tab_requests,
        asking,
        prompted,
        phone_files,
        phone_storage,
    };
    Ok(Outside::new(shell, facts, local_capabilities(home), made))
}

/// Listens for `waos` on the socket `waos-dev-protocol` names, serving it with `served`: installing
/// what it sends, launching an installed app and opening files with one, telling each attach what
/// `asking` says its app's user is asked, and taking computers on the network too while `access`
/// is on; and makes a quit ask every app running first, stopping the server and keeping the
/// engine's days once they agree. Not on a phone: `waos` reaches none yet, and the system ends an
/// app there without asking it.
#[cfg(desktop)]
fn listen_for_the_command(
    app: &mut App,
    served: ServedDomains,
    asking: Arc<AskingNow>,
    access: DeveloperAccess,
) -> Result<()> {
    let socket = waos_dev_protocol::listener_path();
    let (running, engine) = (Arc::clone(&served.running), Arc::clone(&served.engine));
    let server = DevServer::start(served, asking, &socket, access)
        .with_context(|| format!("listening for waos at {}", socket.display()))?;
    tracing::info!(socket = %socket.display(), "WaOS listens for waos");
    quitting::ask_programs_before_quitting(app, running, server, engine);
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
/// opened it, which a program with no window yet shows its next one with; and one about to start
/// as a task of its own has its first window opened among `windows` now, which starts it once it
/// arrives, the library saying so where none opens.
pub(crate) fn bring_opened_programs_forward(
    app: &mut App,
    library: &Entity<Library>,
    windows: &Entity<ViewWindows>,
) {
    let forward = windows.downgrade();
    app.subscribe(library, move |app, opened: &ProgramOpened| {
        if let Some(windows) = forward.upgrade() {
            let (id, token) = (opened.id.as_str(), opened.activation_token.clone());
            windows.update(app, |windows, cx| {
                windows.bring_forward_for_launch(id, token, cx);
            });
        }
    })
    .detach();
    let (first, starting) = (windows.downgrade(), library.downgrade());
    app.subscribe(library, move |app, wanted: &FirstWindowWanted| {
        let (Some(windows), Some(library)) = (first.upgrade(), starting.upgrade()) else {
            return;
        };
        let opened = windows.update(app, |windows, cx| {
            windows.prepare_window(wanted.id.as_str(), &wanted.face, cx)
        });
        library.update(app, |library, cx| match opened {
            Ok(()) => library.start_in_its_window(&wanted.id, windows, cx),
            Err(error) => {
                let why = format!("no window opened for it: {error:?}");
                library.first_window_refused(&wanted.id, &why, cx);
            }
        });
    })
    .detach();
}

/// This computer's capabilities, its sound without the microphone where WaOS's bundle does not
/// say why it records, and its home the one `home` keeps, where WaOS gives it.
fn local_capabilities(home: Option<ThisComputerHome>) -> LocalCapabilities {
    let capabilities = if MICROPHONE_USAGE_DECLARED {
        LocalCapabilities::new()
    } else {
        LocalCapabilities::without_microphone()
    };
    match home {
        Some(home) => capabilities.with_home(Arc::new(move || home.get())),
        None => capabilities,
    }
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
    runs: Arc<RunningApps>,
    tab_requests: UnboundedReceiver<TabRequest>,
    view_windows: &Entity<ViewWindows>,
) -> LibraryWindow {
    match place {
        #[cfg(desktop)]
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
        // Android and iOS have no desktop, whose launcher WaOS is there: the library is a phone's.
        #[cfg(mobile)]
        LibraryPlace::Launcher | LibraryPlace::ClosedLauncher => {
            let tabs = Tabs::listen(app, runs, tab_requests);
            phone_deck(app, library, recordings, tabs, view_windows)
        }
        LibraryPlace::Phone => {
            let tabs = Tabs::listen(app, runs, tab_requests);
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

/// The faces of the programs in `packages`, with the identities of the installed ones where a window
/// is an app of its own, their icons for the system among `folders`.
fn program_faces(packages: &Arc<PackageManager>, folders: &AppFolders) -> ProgramFaces {
    ProgramFaces::new(Arc::clone(packages), identities(folders))
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
/// (`Compiler::new`).
fn native_unwind_info_asked() -> bool {
    std::env::var_os("WAOS_NATIVE_UNWIND_INFO").is_some_and(|value| value == "1")
}
