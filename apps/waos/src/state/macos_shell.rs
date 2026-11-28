//! WaOS on macOS ([`MacosShell`]): each running app's windows, menu bar, context menus, file
//! dialogs and notifications are its window process's (`logic::macos_shell`), and so are WaOS's
//! prompts for it, in dialogs of their own there (`state::prompt_dialogs`); the rest of the
//! engine's shell is Waft's and WaOS's own, as on every desktop. What the user opens through
//! macOS, an app's bundle from the Dock or the Finder, or a click on one of its notifications,
//! comes up as the user's opens, which startup opens as the library does.
//!
//! The canvas of each view shown in a window process is WaOS's, made on Waft's compositor, placed
//! in the root of a layer context of its own, which the window shows, with Waft's
//! `MacOs::place_layer_in`, at each size the window reports, at once, or, where the window waits
//! in its resize, with the app's picture at that size in the same transaction under the fence the
//! window sent with it, and let go with its window. What macOS says of an app's notifications
//! goes to the library, whose settings page for the app says when macOS refuses them, and every
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
use futures::stream::{self, BoxStream};
use waft::ui::CompositionLayer;
use waft::{App, AppLifecycleListener, Entity};
use waos::{
    Shell, ShellContextMenus, ShellFileDialogs, ShellMenuBar, ShellNotifications,
    ShellSystemPermissions, ViewPlacement, ViewRequest,
};
use waos_program_window::{CanvasLayer, PromptWindows};
use waos_window_messages::HostedLayers;
use waos_window_messages::os::macos::SendRight;

use super::program_apps::ProgramPrompts;
use crate::logic::asking_now::AskingNow;
use crate::logic::folders::AppFolders;
use crate::logic::macos_shell::{
    self, AppNotifications, AppsFolders, CanvasCall, CanvasKey, CanvasPictures, CanvasPlace,
    CanvasesOnRequest, DialogReport, NoWindowProcesses, NotificationPermissionReading,
    NotificationPermissions, Routes, ToShow, WindowProcesses,
};
use crate::logic::noted_file_dialogs::NotedFileDialogs;
use crate::logic::program_places::{PlacedWhereOpened, ProgramPlaces, TabsOnRequest};
use crate::logic::startup::BeforeTheEngine;
use crate::os::LayerContext;
use crate::state::app_nap_exemption::{self, AppNapExemption};
use crate::state::desktop_host::with_macos;
use crate::state::library::Library;
use crate::state::prompt_dialogs::PromptDialogs;
use crate::system_shell::{SystemShell, UserOpen};
use crate::ui::every_window::ViewContent;

/// WaOS on macOS: its apps' window processes, or why there can be none, and what reaches the parts
/// of WaOS made after the engine, the library and WaOS's prompts.
pub(crate) struct MacosShell {
    /// The parts of the engine's shell every desktop has, Waft's and WaOS's own.
    shared: Shell,
    /// Where the apps' views go, a tab or a window, as each was opened.
    places: ProgramPlaces,
    tabs: TabsOnRequest,
    /// What the apps' users are asked now, a file dialog among it.
    asking: Arc<AskingNow>,
    window_processes: Result<Arc<WindowProcesses>, String>,
    notifications: Option<Arc<AppNotifications>>,
    /// The apps the user opened through macOS, until startup takes them.
    user_opens: Option<UnboundedReceiver<UserOpen>>,
    /// What the dialogs of WaOS's prompts report, for the prompts made after the engine.
    dialog_reports: UnboundedReceiver<DialogReport>,
    /// The activity macOS must not throttle WaOS under, which whether each app's window is in
    /// sight tells.
    app_nap_exemption: Entity<AppNapExemption>,
    /// What the window processes said macOS said of the apps' notifications, and each time they
    /// say it, for the library made after the engine.
    notification_permissions: NotificationPermissions,
    notification_permission_readings: UnboundedReceiver<NotificationPermissionReading>,
}

impl MacosShell {
    /// WaOS on macOS: the window processes of the apps `before`'s package manager installs, each
    /// lasting while its app's instance runs, as `before` tells, their bundles among `folders`,
    /// their views placed as `before`'s places say, in `tabs` or in their
    /// window processes, their file dialogs noted in `asking`, their canvases kept on `app`'s main
    /// thread and whether each of their windows is in sight telling the activity macOS must not
    /// throttle WaOS under from now on; the rest of the engine's shell is `shared`.
    pub(crate) fn new(
        app: &mut App,
        folders: &AppFolders,
        before: &BeforeTheEngine<'_>,
        tabs: TabsOnRequest,
        asking: Arc<AskingNow>,
        shared: Shell,
    ) -> MacosShell {
        let (canvases, canvas_calls) = CanvasesOnRequest::new();
        keep_canvases(app, canvas_calls);
        let (window_presences, presences) = mpsc::unbounded();
        let app_nap_exemption = app_nap_exemption::start(app);
        app_nap_exemption::follow_windows(app, app_nap_exemption.clone(), presences);
        let (dialogs, dialog_reports) = mpsc::unbounded();
        let (notification_permissions, notification_permission_readings) =
            NotificationPermissions::new();
        let routes = Routes {
            canvases: Arc::new(canvases),
            dialog_reports: dialogs,
            notification_permissions: notification_permissions.clone(),
            window_presences,
        };
        let (opens, user_opens) = mpsc::unbounded();
        let apps_folders = AppsFolders {
            data: folders.data.clone(),
            installed: folders.installed_apps.clone(),
        };
        let window_processes = macos_shell::start(
            apps_folders,
            Arc::clone(before.packages),
            before.running.clone(),
            routes,
            opens,
        )
        .map_err(|error| {
            let why = format!("{error:#}");
            tracing::error!("no program's windows can be shown: {why}");
            why
        });
        let notifications = window_processes.as_ref().ok().map(|window_processes| {
            Arc::new(AppNotifications::new(
                Arc::clone(window_processes),
                notification_permissions.clone(),
            ))
        });
        MacosShell {
            shared,
            places: before.places.clone(),
            tabs,
            asking,
            window_processes,
            notifications,
            user_opens: Some(user_opens),
            app_nap_exemption,
            dialog_reports,
            notification_permissions,
            notification_permission_readings,
        }
    }

    /// From now on, what macOS says of an app's notifications goes to `library`, and every app's
    /// window process that runs reads it again each time WaOS comes to the front; answers where
    /// the apps' prompts are shown: in dialogs of their window processes, launched for a prompt
    /// when they do not run, or by `own_windows` where there are none.
    pub(crate) fn follow(
        self,
        app: &mut App,
        library: &Entity<Library>,
        own_windows: Rc<PromptWindows>,
    ) -> ProgramPrompts {
        let MacosShell {
            window_processes,
            notifications,
            dialog_reports,
            notification_permissions,
            notification_permission_readings,
            ..
        } = self;
        let Ok(window_processes) = window_processes else {
            return in_own_windows(own_windows);
        };
        library.update(app, |library, _| {
            library.read_notification_permissions(notification_permissions)
        });
        tell_system_permissions(app, library, notification_permission_readings);
        if let Some(notifications) = notifications {
            read_system_permissions_at_the_front(app, notifications);
        }
        prompts_in_apps(app, &window_processes, dialog_reports, own_windows)
    }

    /// The activity macOS must not throttle WaOS under, for what else keeps it: the apps that
    /// compile and start.
    pub(crate) fn app_nap_exemption(&self) -> Entity<AppNapExemption> {
        self.app_nap_exemption.clone()
    }

    /// Where the views of the apps go: a tab or `windows`, as each was opened.
    fn placed_where_opened(&self, windows: Arc<dyn ViewPlacement>) -> Arc<dyn ViewPlacement> {
        Arc::new(PlacedWhereOpened {
            places: self.places.clone(),
            tabs: self.tabs.clone(),
            windows,
        })
    }
}

impl SystemShell for MacosShell {
    /// The engine's shell on macOS: an app's windows, menu bar, context menus and file dialogs
    /// are its window process's, its notifications posted there under its bundle, and macOS's
    /// permission for them read there; where there can be no window processes, every window an
    /// app asks for fails with why, and none of those parts is there.
    fn shell(&self) -> Shell {
        let window_processes = match &self.window_processes {
            Ok(window_processes) => window_processes,
            Err(why) => {
                return Shell {
                    views: self.placed_where_opened(Arc::new(NoWindowProcesses::new(why.clone()))),
                    system_permissions: None,
                    menu_bar: None,
                    context_menus: None,
                    file_dialogs: None,
                    notifications: None,
                    ..self.shared.clone()
                };
            }
        };
        let file_dialogs = NotedFileDialogs {
            dialogs: Arc::clone(window_processes) as Arc<dyn ShellFileDialogs>,
            asking: Arc::clone(&self.asking),
        };
        Shell {
            views: self.placed_where_opened(Arc::clone(window_processes) as Arc<dyn ViewPlacement>),
            system_permissions: (self.notifications.clone())
                .map(|notifications| notifications as Arc<dyn ShellSystemPermissions>),
            menu_bar: Some(Arc::clone(window_processes) as Arc<dyn ShellMenuBar>),
            context_menus: Some(Arc::clone(window_processes) as Arc<dyn ShellContextMenus>),
            file_dialogs: Some(Arc::new(file_dialogs) as Arc<dyn ShellFileDialogs>),
            notifications: (self.notifications.clone())
                .map(|notifications| notifications as Arc<dyn ShellNotifications>),
            ..self.shared.clone()
        }
    }

    /// The apps the user opened from the Dock or the Finder, and those macOS launched for a click
    /// on one of their notifications, each waiting until it is read; none after the first call.
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen> {
        match self.user_opens.take() {
            Some(user_opens) => user_opens.boxed(),
            None => stream::empty().boxed(),
        }
    }
}

/// Tells `library` each thing macOS says of a program's notifications, in order, on the main
/// thread.
fn tell_system_permissions(
    app: &mut App,
    library: &Entity<Library>,
    mut readings: UnboundedReceiver<NotificationPermissionReading>,
) {
    let library = library.downgrade();
    app.spawn(async move |cx| {
        while let Some(reading) = readings.next().await {
            let Some(library) = library.upgrade() else {
                break;
            };
            cx.update(|app| {
                tracing::info!(?reading, "the system said of a program's notifications");
                library.update(app, |library, cx| library.notification_permission_read(cx))
            });
        }
    });
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
) -> ProgramPrompts {
    let Some(compositor) = app.platform().compositor() else {
        return in_own_windows(own_windows);
    };
    let dialogs = PromptDialogs::new(app, Arc::clone(window_processes), compositor, reports);
    let shown = dialogs.shown();
    let views: ViewContent = Rc::new(move |app, view| shown.read(app).content_of(view));
    ProgramPrompts {
        placement: Rc::new(dialogs),
        views: Some(views),
    }
}

/// The programs' prompts in WaOS's own windows, by `own_windows`, which add no views of their
/// own.
fn in_own_windows(own_windows: Rc<PromptWindows>) -> ProgramPrompts {
    ProgramPrompts {
        placement: own_windows,
        views: None,
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
