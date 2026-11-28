//! Where the programs' windows go, and their menu bars, context menus, file dialogs and
//! notifications with them, tied to the application. On macOS each program's windows are an app of
//! its own (`logic::macos_shell`), started as WaOS starts, opening the programs the user opens the
//! app of with the library's runs, and keeping an app for each program the package manager has
//! installed, following its changes,
//! which shows the program's menu bar, opens its context menus and file dialogs and posts its
//! notifications, each reaching the app through its getter (`AppReach`), what macOS says of a
//! program's notifications going to the library, whose settings page for the program says when
//! macOS refuses them, and every running program's app reading it again each time WaOS comes to
//! the front, where the user comes back from System Settings, the page having sent them there;
//! the canvas of each view shown there is WaOS's, made on Waft's compositor,
//! placed in the root of a layer context of its own, which the app's window shows, with
//! Waft's `MacOs::place_layer_in`, at each size the window reports, at once, or, where the window
//! waits in its resize, with the program's picture at that size in the same transaction under the
//! fence the window sent with it, and let go with its window; and WaOS's prompts for a program are
//! shown in dialogs of their own there, its app launched for a prompt when it does not run
//! (`state::prompt_dialogs`). Elsewhere a program's windows are WaOS's own, `waos-program-window`'s
//! in WaOS's process, its context menus and file dialogs open in them, and so are its prompts',
//! `PromptWindows`; its notifications are WaOS's, posted under WaOS's name; a program has no menu
//! bar there, those desktops having none.
//!
//! The invariant a later editor will break: macOS has no second route. Where the programs' apps
//! cannot be, as when WaOS finds no window library or loader, WaOS says why as an error, and every
//! window a program asks for fails with it; WaOS shows no program's windows itself.

#[cfg(target_os = "macos")]
pub(crate) use macos::*;
#[cfg(not(target_os = "macos"))]
pub(crate) use other::*;

use std::rc::Rc;

use waos_program_window::PromptPlacement;

use crate::ui::every_window::ViewContent;

/// Where the programs' prompts are shown, and what goes in the views of WaOS's own that showing
/// them adds, where it adds any.
pub(crate) struct ProgramPrompts {
    pub(crate) placement: Rc<dyn PromptPlacement>,
    pub(crate) views: Option<ViewContent>,
}

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;

    use futures::StreamExt;
    use futures::channel::mpsc::{self, UnboundedReceiver};
    use waft::ui::CompositionLayer;
    use waft::{App, AppLifecycleListener, Entity};
    use waos::{
        ShellContextMenus, ShellFileDialogs, ShellMenuBar, ShellNotifications,
        ShellSystemPermissions, ViewPlacement, ViewRequest,
    };
    use waos_program_window::{CanvasLayer, PromptWindows};
    use waos_services::packages::PackageManager;
    use waos_window_messages::HostedLayers;
    use waos_window_messages::os::macos::SendRight;

    use super::ProgramPrompts;
    use crate::logic::app_starter::AppStarter;
    use crate::logic::folders::AppFolders;
    use crate::logic::macos_shell::{
        AppNotifications, AppsFolders, CanvasCall, CanvasKey, CanvasPictures, CanvasPlace,
        CanvasesOnRequest, DialogReport, NoProgramApps, NotificationPermissionReading,
        NotificationPermissions, ProgramApps, ToShow, WindowPresence,
    };
    use crate::logic::placement::WindowsOnRequest;
    use crate::logic::running_apps::RunningApps;
    use crate::os::LayerContext;
    use crate::state::app_nap_exemption::{self, AppNapExemption};
    use crate::state::desktop_host::with_macos;
    use crate::state::library::Library;
    use crate::state::prompt_dialogs::PromptDialogs;
    use crate::ui::every_window::ViewContent;

    /// The programs' apps, or why there can be none, with their notifications; and where the
    /// calls on their views' canvases, the reports of their prompts' dialogs, what macOS says of
    /// their notifications and whether each of their windows is in sight arrive.
    pub(crate) struct Started {
        apps: Result<Arc<ProgramApps>, String>,
        notifications: Option<Arc<AppNotifications>>,
        canvas_calls: UnboundedReceiver<CanvasCall>,
        dialog_reports: UnboundedReceiver<DialogReport>,
        notification_permission_readings: UnboundedReceiver<NotificationPermissionReading>,
        window_presences: UnboundedReceiver<WindowPresence>,
    }

    /// The programs' apps, listening for them, over `packages`; or why there can be none, said as an
    /// error.
    pub(crate) fn start(folders: &AppFolders, packages: &Arc<PackageManager>) -> Started {
        let apps_folders = AppsFolders {
            data: folders.data.clone(),
            installed: folders.installed_apps.clone(),
        };
        let (canvases, canvas_calls) = CanvasesOnRequest::new();
        let (dialogs, dialog_reports) = mpsc::unbounded();
        let (notification_permissions, notification_permission_readings) =
            NotificationPermissions::new();
        let (presence, window_presences) = mpsc::unbounded();
        let apps = ProgramApps::start(
            apps_folders,
            Arc::clone(packages),
            canvases,
            dialogs,
            notification_permissions,
            presence,
        )
        .map_err(|error| {
            let why = format!("{error:#}");
            tracing::error!("no program's windows can be shown: {why}");
            why
        });
        let notifications = apps.as_ref().ok().map(|apps| {
            Arc::new(AppNotifications::new(
                apps.reach(),
                apps.notification_permissions(),
            ))
        });
        Started {
            apps,
            notifications,
            canvas_calls,
            dialog_reports,
            notification_permission_readings,
            window_presences,
        }
    }

    /// Where the engine places the programs' windows: in their apps, never in WaOS's own windows,
    /// which `_own_windows` would make; where there are no apps, nowhere, with why.
    pub(crate) fn placement(
        started: &Started,
        _own_windows: WindowsOnRequest,
    ) -> Arc<dyn ViewPlacement> {
        match &started.apps {
            Ok(apps) => apps.reach() as Arc<dyn ViewPlacement>,
            Err(why) => Arc::new(NoProgramApps::new(why.clone())),
        }
    }

    /// The programs' menu bars: each its app's, which macOS shows while the app is in front; none
    /// where there are no apps, and so no program's windows.
    pub(crate) fn menu_bar(started: &Started) -> Option<Arc<dyn ShellMenuBar>> {
        let apps = started.apps.as_ref().ok()?;
        Some(apps.reach() as Arc<dyn ShellMenuBar>)
    }

    /// Where the programs' context menus open: in their apps, never in WaOS's own windows, where
    /// `_own` would open them; none where there are no apps.
    pub(crate) fn context_menus(
        started: &Started,
        _own: Option<Arc<dyn ShellContextMenus>>,
    ) -> Option<Arc<dyn ShellContextMenus>> {
        let apps = started.apps.as_ref().ok()?;
        Some(apps.reach() as Arc<dyn ShellContextMenus>)
    }

    /// Where the programs' file dialogs open: in their apps, launched for a dialog if they do not
    /// run, never in WaOS's process, where `_own` would open them; none where there are no apps.
    pub(crate) fn file_dialogs(
        started: &Started,
        _own: Option<Arc<dyn ShellFileDialogs>>,
    ) -> Option<Arc<dyn ShellFileDialogs>> {
        let apps = started.apps.as_ref().ok()?;
        Some(apps.reach() as Arc<dyn ShellFileDialogs>)
    }

    /// Where the programs' notifications are posted: in their apps, under each program's bundle,
    /// launched for a notification if they do not run, never under WaOS's name, as `_own` would
    /// post them; none where there are no apps.
    pub(crate) fn notifications(
        started: &Started,
        _own: Option<Arc<dyn ShellNotifications>>,
    ) -> Option<Arc<dyn ShellNotifications>> {
        let notifications = started.notifications.clone()?;
        Some(notifications as Arc<dyn ShellNotifications>)
    }

    /// What macOS itself allows of what a program's user allowed it: its permission for each
    /// program's app's notifications; none where there are no apps.
    pub(crate) fn system_permissions(started: &Started) -> Option<Arc<dyn ShellSystemPermissions>> {
        let notifications = started.notifications.clone()?;
        Some(notifications as Arc<dyn ShellSystemPermissions>)
    }

    /// From now on, the canvases of the views in the programs' apps are kept on the main thread,
    /// the programs whose app the user opens alone are opened by `starter`, what macOS says of a program's notifications goes to `library`,
    /// every running program's app reads it again each time WaOS comes to the front, and whether
    /// each of their windows is in sight tells `activity`; answers where the programs' prompts are
    /// shown: in dialogs of their apps, launched for a prompt when they do not run, or by
    /// `own_windows` where there are no apps.
    pub(crate) fn follow(
        app: &mut App,
        started: Started,
        running: &Arc<RunningApps>,
        starter: Arc<AppStarter>,
        library: &Entity<Library>,
        own_windows: Rc<PromptWindows>,
        activity: &Entity<AppNapExemption>,
    ) -> ProgramPrompts {
        let Started {
            apps,
            notifications,
            canvas_calls,
            dialog_reports,
            notification_permission_readings,
            window_presences,
        } = started;
        let Ok(apps) = apps else {
            return in_own_windows(own_windows);
        };
        keep_canvases(app, canvas_calls);
        let permissions = apps.notification_permissions();
        library.update(app, |library, _| {
            library.read_notification_permissions(permissions)
        });
        tell_system_permissions(app, library, notification_permission_readings);
        app_nap_exemption::follow_windows(app, activity.clone(), window_presences);
        if let Some(notifications) = notifications {
            read_system_permissions_at_the_front(app, notifications, Arc::clone(running));
        }
        let prompts = prompts_in_apps(app, &apps, dialog_reports, own_windows);
        apps.open_with(starter);
        prompts
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

    /// From now on, each time WaOS comes to the front, the app of every program running in `runs`
    /// reads macOS's permission for its notifications again, through `notifications`: macOS says
    /// nothing when the user changes it in System Settings, and a user WaOS's settings page sent
    /// there comes back to WaOS, not to the program's app.
    fn read_system_permissions_at_the_front(
        app: &mut App,
        notifications: Arc<AppNotifications>,
        runs: Arc<RunningApps>,
    ) {
        let read_again = move |app: &mut App| {
            let (notifications, runs) = (Arc::clone(&notifications), Arc::clone(&runs));
            app.spawn(async move |_cx| {
                notifications.read_system_permissions(&runs.programs().await);
            });
        };
        AppLifecycleListener::new(app).on_resume(app, Rc::new(read_again));
    }

    /// The programs' prompts in dialogs of their apps, as `apps` reaches them, drawn on Waft's
    /// compositor, their dialogs reporting on `reports`; by `own_windows` where Waft's platform
    /// has no compositor, as where there are no apps, since no layer of WaOS's can reach an app.
    fn prompts_in_apps(
        app: &mut App,
        apps: &ProgramApps,
        reports: UnboundedReceiver<DialogReport>,
        own_windows: Rc<PromptWindows>,
    ) -> ProgramPrompts {
        let Some(compositor) = app.platform().compositor() else {
            return in_own_windows(own_windows);
        };
        let dialogs = PromptDialogs::new(app, apps.reach(), compositor, reports);
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
                            program = process.program().name(),
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
        fn place(
            &self,
            app: &App,
            place: &CanvasPlace,
            fence: Option<SendRight>,
            with: impl FnOnce(),
        ) {
            place_in_context(app, &self.context, &self.layer, place, fence, with);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use std::rc::Rc;
    use std::sync::Arc;

    use waos::{
        ShellContextMenus, ShellFileDialogs, ShellMenuBar, ShellNotifications,
        ShellSystemPermissions, ViewPlacement,
    };
    use waos_program_window::PromptWindows;
    use waos_services::packages::PackageManager;

    use super::ProgramPrompts;
    use crate::logic::folders::AppFolders;
    use crate::logic::placement::WindowsOnRequest;
    #[cfg(any(windows, target_os = "linux"))]
    use crate::logic::program_entries::ProgramEntries;
    #[cfg(any(windows, target_os = "linux"))]
    use crate::logic::program_identities::Identities;

    /// No program has an app of its own here, beyond its windows' identity; each installed one's
    /// entry where the system lists its apps, on Windows and Linux, over the store.
    pub(crate) struct Started {
        entries: Option<KeptEntries>,
    }

    /// The installed programs' entries, and the store they follow.
    struct KeptEntries {
        #[cfg(any(windows, target_os = "linux"))]
        entries: ProgramEntries,
        #[cfg(any(windows, target_os = "linux"))]
        packages: Arc<PackageManager>,
    }

    /// The installed programs' entries over `packages`, among `folders`, where the system lists apps
    /// by entries WaOS writes.
    pub(crate) fn start(folders: &AppFolders, packages: &Arc<PackageManager>) -> Started {
        Started {
            entries: kept_entries(folders, packages),
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    fn kept_entries(folders: &AppFolders, packages: &Arc<PackageManager>) -> Option<KeptEntries> {
        let identities = Identities::of_this_waos(folders.program_icons.clone())?;
        Some(KeptEntries {
            entries: ProgramEntries::of_this_system(folders, identities),
            packages: Arc::clone(packages),
        })
    }

    /// None: Android lists an app by its package, which WaOS's programs have none of.
    #[cfg(not(any(windows, target_os = "linux")))]
    fn kept_entries(_folders: &AppFolders, _packages: &Arc<PackageManager>) -> Option<KeptEntries> {
        None
    }

    /// From now on, the installed programs' entries in `kept` match the package manager's record,
    /// now and at each change it announces.
    #[cfg(any(windows, target_os = "linux"))]
    fn follow_installed(kept: KeptEntries) {
        if let Err(error) = kept.entries.follow(kept.packages) {
            tracing::warn!(%error, "the programs' entries do not follow the store");
        }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn follow_installed(_kept: KeptEntries) {}

    /// Where the engine places the programs' windows: in WaOS's own windows.
    pub(crate) fn placement(
        _started: &Started,
        own_windows: WindowsOnRequest,
    ) -> Arc<dyn ViewPlacement> {
        Arc::new(own_windows)
    }

    /// No program's menu bar: a program's windows are WaOS's own here, and these desktops have no
    /// menu bar.
    pub(crate) fn menu_bar(_started: &Started) -> Option<Arc<dyn ShellMenuBar>> {
        None
    }

    /// Where the programs' context menus open: at their views in WaOS's own windows, by `own`.
    pub(crate) fn context_menus(
        _started: &Started,
        own: Option<Arc<dyn ShellContextMenus>>,
    ) -> Option<Arc<dyn ShellContextMenus>> {
        own
    }

    /// Where the programs' file dialogs open: on their own or as sheets on WaOS's own windows, by
    /// `own`.
    pub(crate) fn file_dialogs(
        _started: &Started,
        own: Option<Arc<dyn ShellFileDialogs>>,
    ) -> Option<Arc<dyn ShellFileDialogs>> {
        own
    }

    /// Where the programs' notifications are posted: under WaOS's name, by `own`.
    pub(crate) fn notifications(
        _started: &Started,
        own: Option<Arc<dyn ShellNotifications>>,
    ) -> Option<Arc<dyn ShellNotifications>> {
        own
    }

    /// None: the system keeps no permission of its own for a program here, every program's
    /// notifications being WaOS's, posted under its name.
    pub(crate) fn system_permissions(
        _started: &Started,
    ) -> Option<Arc<dyn ShellSystemPermissions>> {
        None
    }

    /// From now on, the installed programs' entries match what the package manager has installed; answers
    /// where the programs' prompts are shown: in WaOS's own windows, by `own_windows`.
    pub(crate) fn follow(started: Started, own_windows: Rc<PromptWindows>) -> ProgramPrompts {
        if let Some(kept) = started.entries {
            follow_installed(kept);
        }
        ProgramPrompts {
            placement: own_windows,
            views: None,
        }
    }
}
