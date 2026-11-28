//! WaOS's shell on macOS: each running app's windows, menu bar, context menus, file dialogs and
//! notifications are its window process's (`logic::macos_shell`), and so are WaOS's prompts for
//! it, in dialogs of their own there (`state::prompt_dialogs`); the rest of the engine's shell is
//! Waft's and WaOS's own, as on every desktop. What the user opens through macOS, an app's bundle
//! from the Dock or the Finder, or a click on one of its notifications, its window process asks
//! app management to open.
//!
//! The canvas of each view shown in a window process is WaOS's, made on Waft's compositor, placed
//! in the root of a layer context of its own, which the window shows, with Waft's
//! `MacOs::place_layer_in`, at each size the window reports, at once, or, where the window waits
//! in its resize, with the app's picture at that size in the same transaction under the fence the
//! window sent with it, and let go with its window. What macOS says of an app's notifications
//! goes up to the library, whose settings page for the app says when macOS refuses them, and every
//! running app's window process reads it again each time WaOS comes to the front, where the user
//! comes back from System Settings, the page having sent them there.
//!
//! The invariants a later editor will break: macOS has no second route. Where the window
//! processes cannot be, as when WaOS finds no window library or loader, WaOS says why as an
//! error, and every window an app asks for fails with it; WaOS shows no app's windows itself. And
//! the canvases are made, placed and let go on the main thread alone, in the order asked, by the
//! task the shell starts as it is made, which ends once nothing is left to ask it.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver};
use waft::ui::CompositionLayer;
use waft::{App, AppLifecycleListener};
use waos::{
    Shell, ShellCameras, ShellContextMenus, ShellFileDialogs, ShellImageCodecs, ShellMenuBar,
    ShellNotifications, ShellSystemPermissions, ViewPlacement, ViewRequest,
};
use waos_program_window::{CanvasLayer, PromptWindows};
use waos_window_messages::HostedLayers;
use waos_window_messages::os::macos::SendRight;

use crate::logic::folders::AppFolders;
use crate::logic::macos_shell::{
    self, AppNotifications, AppsFolders, CanvasCall, CanvasKey, CanvasPictures, CanvasPlace,
    CanvasesOnRequest, DialogReport, NoWindowProcesses, Routes, ToShow, WindowProcesses,
};
use crate::logic::noted_file_dialogs::NotedFileDialogs;
use crate::logic::program_identities::Identities;
use crate::logic::program_places::PlacedWhereOpened;
use crate::logic::simulated_cameras::SimulatedCameras;
use crate::os::LayerContext;
use crate::state::app_nap_exemption;
use crate::state::desktop_host::with_macos;
use crate::state::prompt_dialogs::PromptDialogs;
use crate::state::shells::{AppPrompts, BuiltShell, SharedParts};
use crate::ui::every_window::ViewContent;

/// Whether WaOS's simulated camera is there: yes.
pub(crate) const SIMULATED_CAMERA: bool = true;

/// Whether each app may have a place of its own beside WaOS's, so where apps open is the user's
/// choice: no, an app's windows are its window process's.
pub(crate) const APPS_OPEN_AS_CHOSEN: bool = false;

/// WaOS's shell on macOS: the window processes of the apps `shared`'s package manager installs,
/// each lasting while its app's instance runs, as app management tells, their bundles among
/// `shared`'s folders, their views placed as `shared`'s places say, in `shared`'s tabs or in their
/// window processes, their file dialogs noted in `shared`'s asking, their canvases kept on `app`'s
/// main thread, what macOS says of their notifications told to `shared`'s record of it, and the
/// activity macOS must not throttle WaOS under held while an app's window is in sight, while an
/// app is installed or optimized and while one opens; the apps' prompts in dialogs of their window
/// processes. Where there can be no window processes, every window an app asks for fails with
/// why, none of their parts is there, and the prompts are in WaOS's own windows. The rest of the
/// engine's shell is `shared`'s.
pub(crate) fn shell(app: &mut App, shared: SharedParts<'_>) -> BuiltShell {
    let (canvases, canvas_calls) = CanvasesOnRequest::new();
    keep_canvases(app, canvas_calls);
    let (window_presences, presences) = mpsc::unbounded();
    let app_nap_exemption = app_nap_exemption::start(app);
    app_nap_exemption::follow_windows(app, app_nap_exemption.clone(), presences);
    app_nap_exemption::follow_sessions(app, app_nap_exemption.clone(), shared.packages.sessions());
    let opening = shared.app_management.opening();
    app_nap_exemption::follow_starts(app, app_nap_exemption, opening);
    let (dialogs, dialog_reports) = mpsc::unbounded();
    let routes = Routes {
        canvases: Arc::new(canvases),
        dialog_reports: dialogs,
        notification_permissions: shared.notification_permissions.clone(),
        window_presences,
    };
    let apps_folders = AppsFolders {
        data: shared.folders.data.clone(),
        installed: shared.folders.installed_apps.clone(),
    };
    let packages = Arc::clone(&shared.packages);
    let app_management = Arc::clone(&shared.app_management);
    let started = macos_shell::start(apps_folders, packages, app_management, routes);
    let window_processes = match started {
        Ok(window_processes) => window_processes,
        Err(error) => {
            let why = format!("{error:#}");
            tracing::error!("no program's windows can be shown: {why}");
            return without_window_processes(why, shared);
        }
    };
    let notifications = Arc::new(AppNotifications::new(
        Arc::clone(&window_processes),
        shared.notification_permissions.clone(),
    ));
    read_system_permissions_at_the_front(app, Arc::clone(&notifications));
    let prompts = prompts_in_apps(
        app,
        &window_processes,
        dialog_reports,
        shared.prompt_windows,
    );
    let file_dialogs = NotedFileDialogs {
        dialogs: Arc::clone(&window_processes) as Arc<dyn ShellFileDialogs>,
        asking: Arc::clone(&shared.asking),
    };
    let views = PlacedWhereOpened {
        places: shared.places,
        tabs: shared.tabs,
        windows: Arc::clone(&window_processes) as Arc<dyn ViewPlacement>,
    };
    let shell = Shell {
        views: Arc::new(views),
        system_permissions: Some(Arc::clone(&notifications) as Arc<dyn ShellSystemPermissions>),
        menu_bar: Some(Arc::clone(&window_processes) as Arc<dyn ShellMenuBar>),
        context_menus: Some(Arc::clone(&window_processes) as Arc<dyn ShellContextMenus>),
        file_dialogs: Some(Arc::new(file_dialogs) as Arc<dyn ShellFileDialogs>),
        notifications: Some(notifications as Arc<dyn ShellNotifications>),
        ..shared.shell
    };
    BuiltShell {
        shell,
        prompts,
        shortcuts: None,
    }
}

/// The shell where there can be no window processes, for `why`: every window an app asks for
/// fails with it, none of their parts is there, and the apps' prompts are in WaOS's own windows.
fn without_window_processes(why: String, shared: SharedParts<'_>) -> BuiltShell {
    let views = PlacedWhereOpened {
        places: shared.places,
        tabs: shared.tabs,
        windows: Arc::new(NoWindowProcesses::new(why)),
    };
    let shell = Shell {
        views: Arc::new(views),
        system_permissions: None,
        menu_bar: None,
        context_menus: None,
        file_dialogs: None,
        notifications: None,
        ..shared.shell
    };
    BuiltShell::in_own_windows(shell, shared.prompt_windows)
}

/// None: an app's windows are its window process's, which has its bundle's identity.
pub(crate) fn app_identities(_folders: &AppFolders) -> Option<Identities> {
    None
}

/// WaOS's simulated cameras, which encode their photos with `codecs`.
pub(crate) fn simulated_cameras(
    codecs: Option<Arc<dyn ShellImageCodecs>>,
) -> Option<Arc<dyn ShellCameras>> {
    Some(Arc::new(SimulatedCameras::new(codecs)))
}

/// From now on, each time WaOS comes to the front, every app's window process that runs reads
/// macOS's permission for its notifications again, through `notifications`: macOS says nothing
/// when the user changes it in System Settings, and a user WaOS's settings page sent there comes
/// back to WaOS, not to the app's window process.
fn read_system_permissions_at_the_front(app: &mut App, notifications: Arc<AppNotifications>) {
    let read_again = move |_app: &mut App| notifications.read_system_permissions();
    AppLifecycleListener::new(app).on_resume(app, Rc::new(read_again));
}

/// The programs' prompts in dialogs of their apps, as `window_processes` reach them, drawn on
/// Waft's compositor, their dialogs reporting on `reports`; by `own_windows` where Waft's platform
/// has no compositor, as where there are no apps, since no layer of WaOS's can reach an app.
fn prompts_in_apps(
    app: &mut App,
    window_processes: &Arc<WindowProcesses>,
    reports: UnboundedReceiver<DialogReport>,
    own_windows: Rc<PromptWindows>,
) -> AppPrompts {
    let Some(compositor) = app.platform().compositor() else {
        return AppPrompts::in_own_windows(own_windows);
    };
    let dialogs = PromptDialogs::new(app, Arc::clone(window_processes), compositor, reports);
    let shown = dialogs.shown();
    let views: ViewContent = Rc::new(move |app, view| shown.read(app).content_of(view));
    AppPrompts {
        placement: Rc::new(dialogs),
        views: Some(views),
    }
}

/// Places `layer` in `context`'s root at `place`, as a view's frame places a layer in its
/// window, the root sized with it, under `fence`, and runs `with` in the same transaction, as
/// the program's picture shown with the layout; `with` runs alone where the layer cannot be
/// placed.
pub(crate) fn place_in_context(
    app: &App,
    context: &LayerContext,
    layer: &Arc<dyn CompositionLayer>,
    place: &CanvasPlace,
    fence: Option<SendRight>,
    with: impl FnOnce(),
) {
    let mut with = Some(with);
    let placed = with_macos(app, |macos| {
        let root_size = (place.root_width, place.root_height);
        context.lay_out(root_size, fence, |root| {
            macos.place_layer_in(root, layer, &place.placement, place.scale);
            if let Some(with) = with.take() {
                with();
            }
        });
    });
    if placed.is_none() {
        tracing::warn!("a layer is placed by Waft's macOS host, which is not this one");
    }
    if let Some(with) = with {
        with();
    }
}

/// Carries out every call on the canvases, in order, on the main thread.
fn keep_canvases(app: &mut App, mut calls: UnboundedReceiver<CanvasCall>) {
    app.spawn(async move |cx| {
        let mut kept = KeptCanvases::default();
        while let Some(call) = calls.next().await {
            cx.update(|app| kept.carry_out(app, call));
        }
    });
}

/// The canvases of the views in the programs' apps, by their keys.
#[derive(Default)]
struct KeptCanvases(HashMap<CanvasKey, KeptCanvas>);

/// The canvas of a view in a program's app: the layer context its window shows, and Waft's
/// layer of the canvas, placed in the context's root.
struct KeptCanvas {
    context: LayerContext,
    layer: Arc<dyn CompositionLayer>,
}

impl KeptCanvases {
    fn carry_out(&mut self, app: &App, call: CanvasCall) {
        match call {
            CanvasCall::Make {
                key,
                process,
                request,
            } => match make_canvas(app, &request) {
                Ok((canvas, kept)) => {
                    let layers = HostedLayers {
                        context_id: kept.context.id(),
                    };
                    tracing::info!(
                        app = %process.app(),
                        window = request.id,
                        context = layers.context_id,
                        "a view's canvas is kept in a layer context for its app's window"
                    );
                    self.0.insert(key, kept);
                    process.canvas_made(key, request, canvas, layers);
                }
                Err(reason) => process.canvas_refused(request, reason),
            },
            CanvasCall::Place {
                key,
                metrics,
                fence,
            } => {
                if let Some(kept) = self.0.get(&key) {
                    kept.place(app, &CanvasPlace::of(&metrics), fence, || {});
                }
            }
            CanvasCall::Show {
                key,
                shown,
                pictures,
            } => {
                let answered = shown.answered();
                self.show(app, key, shown, &pictures);
                pictures.shown_on_main_thread(answered);
            }
            CanvasCall::Remove(key) => {
                self.0.remove(&key);
            }
        }
    }

    /// Shows `shown` on the canvas of `key`, which `pictures` presents to: a picture alone, or
    /// the canvas laid out at the size its window waits for, with the picture of that size, in
    /// one transaction under the window's fence. A picture whose canvas is gone goes to it
    /// all the same, which gives its buffer back.
    fn show(&self, app: &App, key: CanvasKey, shown: ToShow, pictures: &CanvasPictures) {
        match shown {
            ToShow::Picture(picture) => pictures.show(picture),
            ToShow::Resize(resize, picture) => {
                let show = || {
                    if let Some(picture) = picture {
                        pictures.show(picture);
                    }
                };
                match self.0.get(&key) {
                    Some(kept) => {
                        let place = CanvasPlace::of(&resize.metrics);
                        kept.place(app, &place, Some(resize.fence), show);
                    }
                    None => show(),
                }
            }
        }
    }
}

/// The canvas of the window `request` asks for, made on Waft's compositor, and what WaOS
/// keeps of it: a layer context of its own; why not, where Waft's platform has no compositor.
fn make_canvas(
    app: &App,
    request: &ViewRequest,
) -> Result<(Arc<CanvasLayer>, KeptCanvas), &'static str> {
    let compositor = app
        .platform()
        .compositor()
        .ok_or("Waft's platform shows no composition layers")?;
    let canvas = Arc::new(CanvasLayer::make(&*compositor, request.config.background));
    let kept = KeptCanvas {
        context: LayerContext::new(),
        layer: canvas.layer(),
    };
    Ok((canvas, kept))
}

impl KeptCanvas {
    /// Places the canvas in its context's root at `place`, under `fence`, with `with` run in
    /// the same transaction.
    fn place(&self, app: &App, place: &CanvasPlace, fence: Option<SendRight>, with: impl FnOnce()) {
        place_in_context(app, &self.context, &self.layer, place, fence, with);
    }
}
