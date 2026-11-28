//! The menus on the main thread, over Waft's: WaOS's own menu bar (`logic::menu_bar`), set as
//! WaOS starts; the context menus programs open at their views in WaOS's own windows; and the
//! programs' tray icons, each answered once Waft has made it, with the menus it offers, and what
//! the user does to it reported to its program through the listener Waft's binding keeps for the
//! icon. Each is there only where Waft's platform has the place.
//!
//! A program's menu bar is never WaOS's: on macOS it is its app's, as its context menus are
//! (`logic::macos_shell`), and the other desktops have no menu bar. Its tray icons stay WaOS's
//! everywhere, belonging to no window.
//!
//! The invariant: every menu opened is answered once it closes, and at once with none where none
//! could open; every icon asked for is answered once Waft has made it or refused it, and an icon
//! whose program no longer waits for it is let go.

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::ui::{
    Offset, Platform, PopupMenuFuture, TrayIcon, TrayIconError as WaftIconError, TrayIconEvent,
};
use waft::{App, Entity, WidgetsBinding};
use waos::bindings::wasdk::platform::menus::{self, MenuEntry, TrayIconError};
use waos::bindings::wasdk::platform::views::ViewId;
use waos::bindings::wasdk::platform::windows::Point;
use waos::{RunningProgram, ShellContextMenus, ShellTrayIcons, TrayIconReports};
use waos_program_window::{waft_image, waft_menu};

use crate::logic::menu_bar::waos_bar;
use crate::logic::menu_calls::{IconCall, IconKey, IconMenus, MenuCall, MenusOnRequest};
use crate::state::ViewWindows;

/// Waft's menus, served to the engine as its shell's.
pub(crate) struct Menus {
    calls: MenusOnRequest,
    arriving: UnboundedReceiver<MenuCall>,
    has_context_menus: bool,
    has_tray: bool,
}

impl Menus {
    /// Waft's menus on `platform`, to serve to the engine.
    pub(crate) fn of(platform: &dyn Platform) -> Menus {
        let (calls, arriving) = MenusOnRequest::new();
        Menus {
            calls,
            arriving,
            has_context_menus: platform.popup_menus().is_some(),
            has_tray: platform.tray_icons().is_some(),
        }
    }

    /// The context menus at the programs' views in WaOS's own windows, as the engine's shell has
    /// them; none where Waft's platform has none.
    pub(crate) fn context_menus(&self) -> Option<Arc<dyn ShellContextMenus>> {
        self.has_context_menus
            .then(|| Arc::new(self.calls.clone()) as Arc<dyn ShellContextMenus>)
    }

    /// The tray, as the engine's shell has it; none where Waft's platform has none.
    pub(crate) fn tray_icons(&self) -> Option<Arc<dyn ShellTrayIcons>> {
        self.has_tray
            .then(|| Arc::new(self.calls.clone()) as Arc<dyn ShellTrayIcons>)
    }

    /// From now on, WaOS's own bar is shown, where Waft's platform has a menu bar, and the
    /// programs' calls are carried out through Waft's menus, a context menu at a view of
    /// `view_windows`.
    pub(crate) fn serve(self, app: &mut App, view_windows: Entity<ViewWindows>) {
        if let Some(menu_bar) = app.platform().menu_bar() {
            menu_bar.set_menus(&waos_bar());
        }
        let shown = app.new_entity(|_| ShownMenus {
            view_windows,
            icons: HashMap::new(),
        });
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &shown, call));
            }
        });
    }
}

/// What the menus keep on the main thread.
struct ShownMenus {
    /// WaOS's own windows, at whose views the programs' context menus open.
    view_windows: Entity<ViewWindows>,
    /// The programs' tray icons, as Waft holds them.
    icons: HashMap<IconKey, Box<dyn TrayIcon>>,
}

/// Carries out `call`, from a program's thread.
fn take(app: &mut App, shown: &Entity<ShownMenus>, call: MenuCall) {
    match call {
        MenuCall::ContextMenu {
            program,
            view,
            at,
            entries,
            chosen,
        } => {
            let opened = open_context_menu(app, shown, &program, view, at, &entries);
            answer_when_closed(app, opened, chosen);
        }
        MenuCall::AddIcon {
            icon,
            image,
            tooltip,
            reports,
            added,
        } => add_icon(
            app,
            shown,
            icon,
            (&image, tooltip.as_deref()),
            reports,
            added,
        ),
        MenuCall::Icon(icon, call) => carry_out_on_icon(app, shown, icon, call),
    }
}

/// Opens `entries` at `at`, in the physical pixels of `program`'s view `view`; none where the view
/// is gone.
fn open_context_menu(
    app: &mut App,
    shown: &Entity<ShownMenus>,
    program: &RunningProgram,
    view: ViewId,
    (x, y): (f64, f64),
    entries: &[MenuEntry],
) -> Option<PopupMenuFuture> {
    let view_windows = shown.read(app).view_windows.clone();
    let (waft_view, ratio) = view_windows
        .read(app)
        .waft_view_of(app, |shown, shown_view| {
            shown == program && shown_view.id() == view
        })?;
    let at = Offset::new(x / ratio, y / ratio);
    let platform = app.platform();
    Some(
        platform
            .popup_menus()?
            .show(waft_view, at, &waft_menu(entries)),
    )
}

/// Answers `chosen` with the item chosen in `opened` once it closes; at once with none when no
/// menu opened.
fn answer_when_closed(
    app: &mut App,
    opened: Option<PopupMenuFuture>,
    chosen: oneshot::Sender<Option<u32>>,
) {
    let Some(opened) = opened else {
        let _ = chosen.send(None);
        return;
    };
    app.spawn(async move |_cx| {
        // A program that ended meanwhile hears nothing.
        let _ = chosen.send(opened.await);
    });
}

/// Adds the icon `icon`, showing `image` under `tooltip`, to Waft's tray, and answers `added`
/// once Waft has made it, with the menus it offers, or with why it did not; from then on what
/// happens to it is reported into `reports`. An icon whose program no longer waits for it is
/// dropped at once.
fn add_icon(
    app: &mut App,
    shown: &Entity<ShownMenus>,
    icon: IconKey,
    (image, tooltip): (&menus::Image, Option<&str>),
    reports: TrayIconReports,
    added: oneshot::Sender<Result<IconMenus, TrayIconError>>,
) {
    let platform = app.platform();
    let Some(tray) = platform.tray_icons() else {
        let _ = added.send(Err(TrayIconError::Failed("no tray".to_owned())));
        return;
    };
    let adding = tray.add(&waft_image(image), tooltip);
    let shown = shown.clone();
    app.spawn(async move |cx| {
        let answer = match adding.await {
            Ok(waft) => cx.update(|app| Ok(keep_icon(app, &shown, icon, waft, reports))),
            Err(WaftIconError::Failed(reason)) => Err(TrayIconError::Failed(reason)),
        };
        if added.send(answer).is_err() {
            cx.update(|app| remove_icon(app, &shown, icon));
        }
    });
}

/// Keeps Waft's icon `waft` as the program's `icon`, its events reported into `reports`, and
/// answers the menus it offers.
fn keep_icon(
    app: &mut App,
    shown: &Entity<ShownMenus>,
    icon: IconKey,
    waft: Box<dyn TrayIcon>,
    reports: TrayIconReports,
) -> IconMenus {
    let menus = IconMenus {
        attached: waft.attached_menu().is_some(),
        at_icon: waft.menu_at_icon().is_some(),
    };
    WidgetsBinding::instance(app).add_tray_icon_listener(
        app,
        waft.id(),
        std::rc::Rc::new(move |_app, event| report(&reports, event)),
    );
    shown.update(app, |shown, _| shown.icons.insert(icon, waft));
    menus
}

/// Removes the program's icon `icon` from Waft's tray, its events heard no more.
fn remove_icon(app: &mut App, shown: &Entity<ShownMenus>, icon: IconKey) {
    let Some(removed) = shown.update(app, |shown, _| shown.icons.remove(&icon)) else {
        return;
    };
    WidgetsBinding::instance(app).remove_tray_icon_listener(app, removed.id());
}

/// Carries out what the program asked of its icon `icon`, while Waft holds it.
fn carry_out_on_icon(app: &mut App, shown: &Entity<ShownMenus>, icon: IconKey, call: IconCall) {
    match call {
        IconCall::SetImage(image) => {
            on_icon(app, shown, icon, |waft| waft.set_image(&waft_image(&image)));
        }
        IconCall::SetTooltip(tooltip) => {
            on_icon(app, shown, icon, |waft| {
                waft.set_tooltip(tooltip.as_deref())
            });
        }
        IconCall::SetAttachedMenu(entries) => {
            on_icon(app, shown, icon, |waft| {
                if let Some(menu) = waft.attached_menu() {
                    menu.set_menu(&waft_menu(&entries));
                }
            });
        }
        IconCall::ShowMenuAtIcon { entries, chosen } => {
            let opened = on_icon(app, shown, icon, |waft| {
                let menu = waft.menu_at_icon()?;
                Some(menu.show(&waft_menu(&entries)))
            });
            answer_when_closed(app, opened.flatten(), chosen);
        }
        IconCall::Remove => remove_icon(app, shown, icon),
    }
}

/// Asks `request` of Waft's icon `icon`; none once it is removed.
fn on_icon<R>(
    app: &App,
    shown: &Entity<ShownMenus>,
    icon: IconKey,
    request: impl FnOnce(&dyn TrayIcon) -> R,
) -> Option<R> {
    let shown = shown.read(app);
    shown.icons.get(&icon).map(|icon| request(icon.as_ref()))
}

/// `event`, as the program hears it.
fn report(reports: &TrayIconReports, event: TrayIconEvent) {
    let at = |position: Offset| Point {
        x: position.dx(),
        y: position.dy(),
    };
    let event = match event {
        TrayIconEvent::Activated { position } => menus::TrayIconEvent::Activated(at(position)),
        TrayIconEvent::ContextMenuRequested { position } => {
            menus::TrayIconEvent::ContextMenuRequested(at(position))
        }
        TrayIconEvent::SecondaryActivated { position } => {
            menus::TrayIconEvent::SecondaryActivated(at(position))
        }
        TrayIconEvent::DoubleClicked => menus::TrayIconEvent::DoubleClicked,
        TrayIconEvent::Scrolled { delta } => menus::TrayIconEvent::Scrolled(delta),
        TrayIconEvent::MenuSelected(id) => return reports.attached_menu_chosen(id),
    };
    reports.event(event);
}
