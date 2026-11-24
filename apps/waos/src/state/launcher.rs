//! The launcher: WaOS on a desktop, a card summoned when a program is to be found or opened and
//! gone once it is, as Spotlight shows its apps (decisions 16 and 17 of
//! `docs/proposals/2026-11-12-phone-programs-and-library.md`), when the rules of
//! `logic::launcher_rules` say.
//!
//! Its window is a view WaOS asks for like any other (decision 19): asked with `request_view` the
//! first time the launcher is summoned, carrying [`LAUNCHER_VIEW`] as its initial data, which WaOS's
//! one builder shows the launcher in (`ui::every_window`, `ui::launcher`). The launcher keeps the
//! window as its view arrives and does to it what it decides (decision 21): shows it in the middle
//! of the screen and focused, minimizes it or hides it; what it decides before the window comes is
//! done as the window comes. Nothing of this waits for the window's content, which is not built while the
//! window is out of sight. What is the app's and not the window's is done here as well:
//! WaOS hidden to hand the focus back on macOS, unless a program's window is on screen to take it,
//! and the library put away, back on its grid with nothing searched, a program waiting for the user
//! to leave its settings page counting it as left.
//!
//! It is summoned at a click on WaOS's Dock icon (macOS reports it as a reopen), at a click on its
//! icon in the menu bar or the tray (`state::waos_tray`), whenever the library asks to be seen, a
//! second start of WaOS among it, and at launch unless WaOS started with it closed; dismissed as a
//! program opens from it, and at Escape, the system's close command or the window losing the
//! focus, unless to the file picker the library opened for a package. Dismissed, it is minimized on Windows and Linux while WaOS is kept in the taskbar, and
//! hidden otherwise, as the user's choice is from the next dismissal on. A minimized launcher the
//! user restores from the taskbar takes the focus, and is summoned by that.
//!
//! The invariant: only the launcher shows, minimizes or hides its window, so whether the rules hold
//! the launcher shown is whether it is on screen.

use std::rc::Rc;

use waft::ui::{
    RequestId, TargetPlatform, ViewError, ViewFocusDirection, ViewFocusEvent, ViewFocusState,
    ViewId, ViewRef,
};
use waft::{
    App, Context, Entity, EventEmitter, Size, WeakEntity, WidgetsBinding, WidgetsBindingObserver,
    WindowBackground, WindowConfig, WindowLevel, WindowRef,
};
use waft_embedder_winit::MacOs;

use crate::logic::launcher_placement::launcher_frame;
use crate::logic::launcher_rules::{Dismissal, LauncherRules, LauncherStep, WhenDismissed};
use crate::state::ViewWindows;
use crate::state::desktop_host::with_macos;
use crate::state::library::{
    Library, LibraryPage, PreferencesChosen, ProgramOpened, WantsToBeSeen,
};

/// The initial data of the launcher's view, which names it to WaOS's one builder.
pub(crate) const LAUNCHER_VIEW: &str = "waos-launcher";

/// The launcher's card, and so its window, in logical points.
const LAUNCHER_SIZE: Size = Size::new(680.0, 460.0);

/// The launcher came on screen: what shows in it takes the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LauncherShown;

/// What the launcher's window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowStep {
    /// Shows, in the middle of the screen the user is on, restored where it was minimized, and
    /// takes the focus.
    Show,
    /// Goes, as the dismissal says.
    Go(Dismissal),
}

/// How round the launcher's card is at its corners, and with it the backdrop behind it.
pub(crate) const CARD_CORNER_RADIUS: f64 = 12.0;

pub(crate) struct Launcher {
    rules: LauncherRules,
    /// The request for the window, while it is on its way.
    asked: Option<RequestId>,
    /// The window, once it has come.
    window: Option<WindowRef>,
    /// Whether the system shows its blur or glass behind the window, where the card is tinted
    /// rather than filled: on macOS, once the window has come.
    backdrop_behind: bool,
    /// What the window is to do as it comes, as last decided before it came.
    waiting: Option<WindowStep>,
    /// The programs' windows, any of which takes the focus a hidden launcher hands back.
    view_windows: Entity<ViewWindows>,
    /// The library the launcher shows, put away as the launcher goes.
    library: WeakEntity<Library>,
}

impl Launcher {
    /// The launcher, not shown, going as the desktop it is on has it go: it is summoned at a click
    /// on WaOS's Dock icon and whenever `library` asks to be seen, and dismissed when a program is
    /// opened from it. Its window is asked for when it is first summoned.
    pub(crate) fn new(
        app: &mut App,
        library: &Entity<Library>,
        view_windows: Entity<ViewWindows>,
    ) -> Entity<Launcher> {
        let kept_in_dock = library.read(app).kept_in_dock();
        let when_dismissed = when_dismissed_on(app.platform().target_platform(), kept_in_dock);
        let launcher = app.new_entity(|_| Launcher {
            rules: LauncherRules::new(when_dismissed),
            asked: None,
            window: None,
            backdrop_behind: false,
            waiting: None,
            view_windows,
            library: library.downgrade(),
        });
        follow_library(app, library, &launcher);
        let watcher = Rc::new(LauncherViewWatcher {
            launcher: launcher.downgrade(),
        });
        WidgetsBinding::instance(app).add_observer(app, watcher);
        summon_on_reopen(app, &launcher);
        launcher
    }

    /// WaOS started with the launcher closed, as `waos run` starts it: where a dismissed
    /// launcher is minimized, its window is made and minimized at once, so WaOS has its button in
    /// the taskbar from the start.
    pub(crate) fn started_closed(&mut self, cx: &mut Context<Launcher>) {
        if self.rules.when_dismissed() == WhenDismissed::Minimized {
            self.ask_for_window(cx);
            self.take_step(
                WindowStep::Go(Dismissal::Minimize),
                "WaOS started with it closed",
                cx,
            );
        }
    }

    /// Something summoned the launcher, as `cause` says.
    pub(crate) fn summoned(&mut self, cause: &str, cx: &mut Context<Launcher>) {
        let step = self.rules.summoned();
        self.carry_out(step, cause, cx);
    }

    /// The user pressed Escape in the launcher, or closed its window.
    pub(crate) fn dismissed(&mut self, cx: &mut Context<Launcher>) {
        let step = self.rules.dismissed_by_user();
        self.carry_out(step, "the user dismissed it", cx);
    }

    /// A program was opened from the launcher.
    fn program_opened(&mut self, cx: &mut Context<Launcher>) {
        let step = self.rules.program_opened();
        self.carry_out(step, "a program was opened from it", cx);
    }

    /// A window of the app gained or lost the focus: the launcher's counts.
    fn view_focus_changed(&mut self, event: ViewFocusEvent, cx: &mut Context<Launcher>) {
        if self.view() != Some(event.view_id) {
            return;
        }
        let was_shown = self.rules.is_shown();
        let picking = self
            .library
            .upgrade()
            .is_some_and(|library| library.read(cx).is_picking());
        let step = self
            .rules
            .focus_changed(event.state == ViewFocusState::Focused, picking);
        if !was_shown && self.rules.is_shown() {
            tracing::info!("the launcher was restored by the user");
        }
        self.carry_out(step, "it lost the focus", cx);
    }

    /// Has the window do what `step` says, `cause` being why, and does what goes with it.
    fn carry_out(&mut self, step: LauncherStep, cause: &str, cx: &mut Context<Launcher>) {
        let window_step = match step {
            LauncherStep::Keep => return,
            LauncherStep::Summon => {
                self.ask_for_window(cx);
                WindowStep::Show
            }
            LauncherStep::Dismiss(dismissal) => {
                self.put_library_away(cx);
                if dismissal
                    == (Dismissal::Hide {
                        hand_back_focus: true,
                    })
                {
                    self.hand_back_focus(cx);
                }
                WindowStep::Go(dismissal)
            }
        };
        self.take_step(window_step, cause, cx);
    }

    /// The window does `step` now, `cause` being why, or as it comes.
    fn take_step(&mut self, step: WindowStep, cause: &str, cx: &mut Context<Launcher>) {
        tracing::info!(?step, cause, "the launcher");
        match self.window.clone() {
            Some(window) => do_step(&window, step, cx),
            None => self.waiting = Some(step),
        }
    }

    /// The library goes back to its grid with nothing searched, and hears it is out of sight.
    fn put_library_away(&self, cx: &mut Context<Launcher>) {
        if let Some(library) = self.library.upgrade() {
            library.update(cx, |library, cx| {
                library.hidden(cx);
                library.show_page(LibraryPage::Home, cx);
                library.search_for("", cx);
            });
        }
    }

    /// Hides WaOS, which hands the focus to the app that had it, unless a program's window is on
    /// screen, which AppKit gives the focus to as the launcher goes; elsewhere hiding the window
    /// hands it back.
    fn hand_back_focus(&self, app: &App) {
        if self.view_windows.read(app).is_empty() {
            with_macos(app, MacOs::hide_application);
        }
    }

    /// Asks for the window, unless it was asked for already or has come. It arrives as every view
    /// does, and does what was decided for it as it comes.
    fn ask_for_window(&mut self, cx: &mut Context<Launcher>) {
        if self.asked.is_some() || self.window.is_some() {
            return;
        }
        match cx
            .platform()
            .request_view(launcher_config(), Some(LAUNCHER_VIEW.to_owned()))
        {
            Ok(asked) => self.asked = Some(asked),
            Err(error) => {
                tracing::warn!("this host makes no windows, so the launcher cannot show: {error}");
            }
        }
    }

    /// The view of the launcher's window, once it has come.
    fn view(&self) -> Option<ViewId> {
        self.window.as_ref().map(|window| window.view().id())
    }

    /// The host added `view`: where it is the launcher's, its window is the launcher's from now on,
    /// and does what was decided for it.
    fn view_added(&mut self, view: &ViewRef, cx: &mut Context<Launcher>) {
        if view.initial_data().as_deref() != Some(LAUNCHER_VIEW) {
            return;
        }
        self.asked = None;
        let Some(window) = view.window() else {
            tracing::warn!("the launcher's view came in no window, so the launcher cannot show");
            return;
        };
        self.backdrop_behind = show_backdrop_behind(&window, cx);
        if let Some(step) = self.waiting.take() {
            do_step(&window, step, cx);
        }
        self.window = Some(window);
        cx.notify();
    }

    /// Whether the system shows its blur or glass behind the launcher's window, so the card is
    /// tinted over it rather than filled.
    pub(crate) fn backdrop_behind(&self) -> bool {
        self.backdrop_behind
    }

    /// The host removed `view`: where it was the launcher's, the next summons asks for another.
    fn view_removed(&mut self, view: ViewId) {
        if self.view() == Some(view) {
            self.window = None;
        }
    }

    /// The host made no window for `request`: where it was the launcher's, the next summons asks
    /// again.
    fn view_request_failed(&mut self, request: RequestId, error: &ViewError) {
        if self.asked == Some(request) {
            tracing::warn!("no window for the launcher: {error}");
            self.asked = None;
        }
    }
}

impl EventEmitter<LauncherShown> for Launcher {}

/// `window` does `step`; a launcher shown says so.
fn do_step(window: &WindowRef, step: WindowStep, cx: &mut Context<Launcher>) {
    match step {
        WindowStep::Show => {
            show_in_the_middle(cx, window);
            cx.emit(LauncherShown);
        }
        WindowStep::Go(Dismissal::Minimize) => {
            window.set_minimized(true);
            window.show();
        }
        WindowStep::Go(Dismissal::Hide { .. }) => window.hide(),
    }
}

/// Shows `window` in the middle of the screen the user is on, restored where it was minimized,
/// and gives it the focus.
fn show_in_the_middle(app: &App, window: &WindowRef) {
    let frame = app.platform().screens().and_then(|screens| {
        launcher_frame(
            LAUNCHER_SIZE,
            &screens.screens(),
            screens.pointer_location(),
        )
    });
    if let Some(frame) = frame {
        window.set_frame(frame, None);
    }
    window.set_minimized(false);
    window.show();
    window
        .view()
        .host()
        .request_focus_change(ViewFocusState::Focused, ViewFocusDirection::Undefined);
}

/// How the launcher goes on a desktop of `platform`, with WaOS kept in its Dock or taskbar where
/// `kept_in_dock` says, as it is where WaOS offers no choice: minimized on Windows and Linux, which
/// show a running app in the taskbar or the dock only through its windows, so WaOS keeps its
/// button there; hidden on macOS, whose Dock shows WaOS whether or not it has a window, and
/// wherever WaOS is not kept there, its icon in the menu bar or the tray summoning it.
fn when_dismissed_on(platform: TargetPlatform, kept_in_dock: Option<bool>) -> WhenDismissed {
    let apps_shown_by_windows = matches!(platform, TargetPlatform::Windows | TargetPlatform::Linux);
    WhenDismissed::on_desktop(apps_shown_by_windows, kept_in_dock.unwrap_or(true))
}

/// The launcher is summoned whenever `library` asks to be seen, dismissed when a program is
/// opened from it, and goes as the user chose WaOS to stay on the desktop.
fn follow_library(app: &mut App, library: &Entity<Library>, launcher: &Entity<Launcher>) {
    let (read, choosing) = (library.downgrade(), launcher.downgrade());
    app.subscribe(library, move |app, _: &PreferencesChosen| {
        if let (Some(library), Some(launcher)) = (read.upgrade(), choosing.upgrade()) {
            let kept_in_dock = library.read(app).kept_in_dock();
            let when_dismissed = when_dismissed_on(app.platform().target_platform(), kept_in_dock);
            launcher.update(app, |launcher, _| {
                launcher.rules.go_when_dismissed(when_dismissed)
            });
        }
    })
    .detach();
    let summoned = launcher.downgrade();
    app.subscribe(library, move |app, _: &WantsToBeSeen| {
        if let Some(launcher) = summoned.upgrade() {
            launcher.update(app, |launcher, cx| {
                launcher.summoned("the library has something to ask or say", cx)
            });
        }
    })
    .detach();
    let dismissed = launcher.downgrade();
    app.subscribe(library, move |app, _: &ProgramOpened| {
        if let Some(launcher) = dismissed.upgrade() {
            launcher.update(app, Launcher::program_opened);
        }
    })
    .detach();
}

/// Has the system show its blur of what is behind `window`, the launcher's, rounded as the
/// card is: Liquid Glass where macOS has it, else the blur a popover has. Answers whether it
/// does, which is on macOS alone.
#[cfg(target_os = "macos")]
fn show_backdrop_behind(window: &WindowRef, app: &App) -> bool {
    use waft_embedder_winit::{
        MacosBackdrop, NSGlassEffectViewStyle, NSVisualEffectMaterial, NSVisualEffectState,
    };

    let backdrop = if MacosBackdrop::has_glass() {
        MacosBackdrop::Glass {
            style: NSGlassEffectViewStyle::Regular,
            tint: None,
            corner_radius: CARD_CORNER_RADIUS,
        }
    } else {
        MacosBackdrop::VisualEffect {
            material: NSVisualEffectMaterial::Popover,
            state: NSVisualEffectState::Active,
            corner_radius: CARD_CORNER_RADIUS,
        }
    };
    with_macos(app, |macos| {
        macos.set_window_backdrop(window, Some(&backdrop))
    })
    .is_some()
}

/// No system this host runs on but macOS has a backdrop to show yet.
#[cfg(not(target_os = "macos"))]
fn show_backdrop_behind(_window: &WindowRef, _app: &App) -> bool {
    false
}

/// A click on WaOS's Dock icon, or WaOS opened again, summons the launcher, where the host says
/// so: on macOS.
fn summon_on_reopen(app: &App, launcher: &Entity<Launcher>) {
    let (launcher, cx) = (launcher.downgrade(), app.to_async());
    with_macos(app, |macos| {
        macos.set_reopen_handler(Some(Rc::new(move |_reopen| {
            let launcher = launcher.clone();
            cx.post(move |app| {
                if let Some(launcher) = launcher.upgrade() {
                    launcher.update(app, |launcher, cx| {
                        launcher.summoned("WaOS was opened again", cx)
                    });
                }
            });
        })));
    });
}

/// A floating card: no border, see-through around its rounded corners, with the system's shadow,
/// above ordinary windows on whichever desktop the user is on, hidden until it is placed.
fn launcher_config() -> WindowConfig {
    WindowConfig {
        title: String::from("WaOS"),
        size: [LAUNCHER_SIZE.width(), LAUNCHER_SIZE.height()],
        decorations: false,
        resizable: false,
        level: WindowLevel::AlwaysOnTop,
        all_desktops: true,
        background: WindowBackground::Transparent,
        shadow: true,
        visible: false,
        ..WindowConfig::default()
    }
}

/// Tells the launcher when a window of the app gains or loses the focus, and when a view arrives,
/// goes or a request for one fails, its window's among them, which the widgets binding hears from
/// the host.
struct LauncherViewWatcher {
    launcher: WeakEntity<Launcher>,
}

impl WidgetsBindingObserver for LauncherViewWatcher {
    fn did_change_view_focus(&self, app: &mut App, event: ViewFocusEvent) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.update(app, |launcher, cx| launcher.view_focus_changed(event, cx));
        }
    }

    fn did_add_view(&self, app: &mut App, view: &ViewRef) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.update(app, |launcher, cx| launcher.view_added(view, cx));
        }
    }

    fn did_remove_view(&self, app: &mut App, view: ViewId) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.update(app, |launcher, _| launcher.view_removed(view));
        }
    }

    fn did_fail_view_request(&self, app: &mut App, request: RequestId, error: &ViewError) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.update(app, |launcher, _| {
                launcher.view_request_failed(request, error)
            });
        }
    }
}
