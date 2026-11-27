//! WaOS's own icon in the menu bar or the tray, while the user chose not to keep WaOS in the Dock
//! or the taskbar ("Keep WaOS in the Dock", in WaOS's preferences): a click on it summons the
//! launcher, which then goes hidden rather than minimized. Where Waft's platform has no tray, there
//! is no icon, which is said. On macOS WaOS keeps its Dock icon meanwhile: the design has it go,
//! WaOS becoming an accessory app, which Waft's host offers no way to make it yet.
//!
//! The invariant: the icon is in the tray exactly while the library says WaOS is not kept in the
//! Dock; an icon Waft makes after the choice changed back is let go at once.

use std::rc::Rc;

use waft::ui::{TrayIcon, TrayIconError, TrayIconEvent};
use waft::{App, Context, Entity, WeakEntity, WidgetsBinding};

use crate::logic::tray_image::waos_tray_image;
use crate::state::launcher::Launcher;
use crate::state::library::{Library, PreferencesChosen};

/// What the system shows when the pointer rests on the icon.
const TOOLTIP: &str = "WaOS";

pub(crate) struct WaosTray {
    /// The icon, while Waft holds it.
    icon: Option<Box<dyn TrayIcon>>,
    /// How many times the icon was asked for or let go, so an icon made for an earlier asking is
    /// let go.
    asked: u64,
    /// Whether the icon is wanted now.
    wanted: bool,
    /// The launcher a click on the icon summons.
    launcher: WeakEntity<Launcher>,
}

impl WaosTray {
    /// WaOS's icon in the tray, there while `library` says WaOS is not kept in the Dock, for as
    /// long as the app runs; a click on it summons `launcher`.
    pub(crate) fn follow(app: &mut App, library: &Entity<Library>, launcher: &Entity<Launcher>) {
        let tray = app.new_entity(|_| WaosTray {
            icon: None,
            asked: 0,
            wanted: false,
            launcher: launcher.downgrade(),
        });
        let kept = library.read(app).kept_in_dock();
        tray.update(app, |tray, cx| tray.follow_choice(kept, cx));
        // The subscription holds the tray for as long as the library lives, which is the app's.
        let read = library.downgrade();
        app.subscribe(library, move |app, _: &PreferencesChosen| {
            if let Some(library) = read.upgrade() {
                let kept = library.read(app).kept_in_dock();
                tray.update(app, |tray, cx| tray.follow_choice(kept, cx));
            }
        })
        .detach();
    }

    /// The icon is in the tray where WaOS is not `kept` in the Dock, and not otherwise; none
    /// where WaOS offers no such choice.
    fn follow_choice(&mut self, kept: Option<bool>, cx: &mut Context<WaosTray>) {
        let wanted = kept == Some(false);
        if wanted == self.wanted {
            return;
        }
        self.wanted = wanted;
        self.asked += 1;
        if wanted {
            self.add(cx);
        } else {
            self.remove(cx);
        }
    }

    /// Asks Waft's tray for the icon, kept once Waft has made it, unless it is no longer wanted.
    fn add(&mut self, cx: &mut Context<WaosTray>) {
        let platform = cx.platform();
        let Some(tray) = platform.tray_icons() else {
            tracing::warn!("this system shows no tray icon, so WaOS has none to be summoned from");
            return;
        };
        let Some(image) = waos_tray_image() else {
            return;
        };
        let adding = tray.add(&image, Some(TOOLTIP));
        let (asked, tray) = (self.asked, cx.entity().downgrade());
        cx.spawn(async move |cx| {
            let made = adding.await;
            cx.update(|app| {
                if let Some(tray) = tray.upgrade() {
                    tray.update(app, |tray, cx| tray.made(asked, made, cx));
                }
            });
        });
    }

    /// Waft answered the icon asked for as `asked`: it is kept and heard, unless the choice changed
    /// since, when it goes again.
    fn made(
        &mut self,
        asked: u64,
        made: Result<Box<dyn TrayIcon>, TrayIconError>,
        cx: &mut Context<WaosTray>,
    ) {
        let icon = match made {
            Ok(icon) => icon,
            Err(error) => {
                tracing::warn!(%error, "WaOS's tray icon");
                return;
            }
        };
        if asked != self.asked {
            return;
        }
        let launcher = self.launcher.clone();
        WidgetsBinding::instance(cx).add_tray_icon_listener(
            cx,
            icon.id(),
            Rc::new(move |app, event| summon_at_a_click(app, &launcher, event)),
        );
        self.icon = Some(icon);
    }

    /// Lets go of the icon, its events heard no more.
    fn remove(&mut self, cx: &mut Context<WaosTray>) {
        if let Some(icon) = self.icon.take() {
            WidgetsBinding::instance(cx).remove_tray_icon_listener(cx, icon.id());
        }
    }
}

/// A click on the icon, or a double click, summons `launcher`.
fn summon_at_a_click(app: &mut App, launcher: &WeakEntity<Launcher>, event: TrayIconEvent) {
    let clicked = matches!(
        event,
        TrayIconEvent::Activated { .. } | TrayIconEvent::DoubleClicked
    );
    if let Some(launcher) = launcher.upgrade().filter(|_| clicked) {
        launcher.update(app, |launcher, cx| {
            launcher.summoned("WaOS's tray icon was clicked", cx)
        });
    }
}
