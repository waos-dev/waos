//! GPUI's platform over wasdk: the app as a whole.
//!
//! What wasdk has, the platform answers with: windows through `views` and `windows`, displays
//! through `screens`, light or dark through `system`, the cursor per view, the clipboard, file
//! dialogs, opening links and files. What it lacks, it answers as `gpui_web` does: a no-op, an
//! error, or GPUI's own fallback (GPUI draws its own prompts when the platform has none).
//!
//! The invariant: GPUI calls the platform from inside its updates, so nothing here calls back into
//! GPUI; what must reach GPUI later (quitting, a menu's choice) goes to the event loop as a notice
//! or is kept for the loop to deliver.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::channel::{mpsc, oneshot};
use gpui::{
    Action, ActivityGuard, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle,
    DummyKeyboardMapper, ForegroundExecutor, Keymap, Menu, MenuItem, OperatingSystem, OwnedMenu,
    PathPromptOptions, Platform, PlatformDisplay, PlatformInfo, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTarget, PlatformTextSystem, PlatformWindow, Task, ThermalState,
    WindowAppearance, WindowKind, WindowParams,
};
use gpui_wgpu::WgpuContext;
use wasdk::system::{self, Brightness};
use wasdk::views::{self, Cursor};
use wasdk::windows::{Point as WindowPoint, Size as WindowSize, WindowConfig};
use wasdk::{clipboard, screens};

use crate::clipboard_text;
use crate::cursor::cursor_of;
use crate::dispatcher::WasdkDispatcher;
use crate::display::WasdkDisplay;
use crate::events::Notice;
use crate::file_dialogs;
use crate::slot::Slot;
use crate::text_system::WasdkTextSystem;
use crate::window::{WasdkWindow, WindowShared};

pub(crate) struct WasdkPlatform {
    pub(crate) dispatcher: Arc<WasdkDispatcher>,
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    context: WgpuContext,
    notices: mpsc::UnboundedSender<Notice>,
    appearance: Rc<Cell<WindowAppearance>>,
    pub(crate) windows: RefCell<Windows>,
    active_window: Cell<Option<AnyWindowHandle>>,
    cursor: Cell<Cursor>,
    cursor_hidden: Cell<bool>,
    /// The menus the app set last, which it reads back to draw them in its window, as GPUI's
    /// Linux and Windows apps do: WaOS's menu bar does not carry them yet.
    menus: RefCell<Vec<OwnedMenu>>,
    callbacks: PlatformCallbacks,
}

/// The windows GPUI opened: those waiting for their view by the request that asks for it, and
/// those shown by their view's id.
#[derive(Default)]
pub(crate) struct Windows {
    pub(crate) pending: HashMap<u64, Rc<WindowShared>>,
    pub(crate) shown: HashMap<u64, Rc<WindowShared>>,
}

#[derive(Default)]
struct PlatformCallbacks {
    quit: Slot<Box<dyn FnMut() -> bool>>,
    displays_changed: Slot<Box<dyn FnMut()>>,
}

thread_local! {
    /// The platform the program's app runs on, for what an app says to it outside GPUI's
    /// traits, as `gpui_ios`'s `current_platform`; none before `run` makes it, or after it ends.
    static CURRENT: RefCell<Weak<WasdkPlatform>> = const { RefCell::new(Weak::new()) };
}

impl WasdkPlatform {
    /// The platform the program's app runs on now.
    pub(crate) fn current() -> Option<Rc<WasdkPlatform>> {
        CURRENT.with_borrow(Weak::upgrade)
    }

    /// From now on, `platform` is the one the program's app runs on.
    pub(crate) fn make_current(platform: &Rc<WasdkPlatform>) {
        CURRENT.set(Rc::downgrade(platform));
    }

    pub(crate) fn new(context: WgpuContext, notices: mpsc::UnboundedSender<Notice>) -> Self {
        let dispatcher = Arc::new(WasdkDispatcher::new(notices.clone()));
        WasdkPlatform {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher.clone()),
            dispatcher,
            text_system: Arc::new(WasdkTextSystem::new()),
            context,
            notices,
            appearance: Rc::new(Cell::new(appearance_of(system::platform_brightness()))),
            windows: RefCell::new(Windows::default()),
            active_window: Cell::new(None),
            cursor: Cell::new(Cursor::Default),
            cursor_hidden: Cell::new(false),
            menus: RefCell::new(Vec::new()),
            callbacks: PlatformCallbacks::default(),
        }
    }

    /// The user changed light or dark.
    pub(crate) fn set_brightness(&self, brightness: Brightness) {
        self.appearance.set(appearance_of(brightness));
    }

    pub(crate) fn set_active_window(&self, window: Option<AnyWindowHandle>) {
        self.active_window.set(window);
    }

    /// The pointer moved: a cursor hidden until then shows again.
    pub(crate) fn pointer_moved(&self) {
        if self.cursor_hidden.replace(false) {
            self.apply_cursor(self.cursor.get());
        }
    }

    fn apply_cursor(&self, cursor: Cursor) {
        for view in views::views() {
            view.set_cursor(cursor);
        }
    }

    pub(crate) fn take_pending_window(&self, request: u64) -> Option<Rc<WindowShared>> {
        self.windows.borrow_mut().pending.remove(&request)
    }

    pub(crate) fn window_shown(&self, view: u64, window: Rc<WindowShared>) {
        self.windows.borrow_mut().shown.insert(view, window);
    }

    pub(crate) fn take_shown_window(&self, view: u64) -> Option<Rc<WindowShared>> {
        let window = self.windows.borrow_mut().shown.remove(&view)?;
        if self.active_window.get() == Some(window.handle) {
            self.active_window.set(None);
        }
        Some(window)
    }

    pub(crate) fn shown_window(&self, view: u64) -> Option<Rc<WindowShared>> {
        self.windows.borrow().shown.get(&view).cloned()
    }

    pub(crate) fn shown_windows(&self) -> Vec<Rc<WindowShared>> {
        self.windows.borrow().shown.values().cloned().collect()
    }

    /// A screen came, went or changed: GPUI reads the displays again.
    pub(crate) fn displays_changed(&self) {
        self.callbacks.displays_changed.call(|callback| callback());
    }

    /// The app is ending: GPUI shuts down, its quit handlers spawned as wasm has them.
    pub(crate) fn quitting(&self) {
        if let Some(mut quit) = self.callbacks.quit.take() {
            quit();
        }
    }

    fn primary_display(&self) -> Rc<dyn PlatformDisplay> {
        screens::screens()
            .first()
            .map(|screen| Rc::new(WasdkDisplay::of(screen)) as Rc<dyn PlatformDisplay>)
            .unwrap_or_else(|| Rc::new(WasdkDisplay::assumed()))
    }
}

impl Platform for WasdkPlatform {
    fn platform_info(&self) -> PlatformInfo {
        PlatformInfo {
            os: operating_system(),
            target: PlatformTarget::Native,
        }
    }

    fn background_executor(&self) -> BackgroundExecutor {
        self.background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        Arc::clone(&self.text_system)
    }

    /// The device is ready before the platform is made, so launching is at once.
    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        on_finish_launching();
    }

    fn quit(&self) {
        let _ = self.notices.unbounded_send(Notice::Quit);
    }

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {}

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        let displays: Vec<Rc<dyn PlatformDisplay>> = screens::screens()
            .iter()
            .map(|screen| Rc::new(WasdkDisplay::of(screen)) as Rc<dyn PlatformDisplay>)
            .collect();
        if displays.is_empty() {
            vec![Rc::new(WasdkDisplay::assumed())]
        } else {
            displays
        }
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(WasdkPlatform::primary_display(self))
    }

    fn on_displays_changed(&self, callback: Box<dyn FnMut()>) {
        self.callbacks.displays_changed.set(callback);
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.active_window.get()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        if let WindowKind::AnchoredPopup(_) = params.kind {
            return Err(gpui::popup::PopupNotSupportedError.into());
        }
        let config = window_config(&params);
        let request = views::request_view(&config, None)
            .map_err(|error| anyhow!("WaOS gave no window: {error:?}"))?;
        let display = self.primary_display();
        let scale = screens::screens()
            .first()
            .map_or(2.0, |screen| screen.scale() as f32);
        let window = WasdkWindow::open(
            handle,
            &params,
            self.context.clone(),
            display,
            scale,
            Rc::clone(&self.appearance),
        );
        self.windows
            .borrow_mut()
            .pending
            .insert(request, window.shared());
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.appearance.get()
    }

    fn open_url(&self, url: &str) {
        if let Some(opener) = wasdk::opener::get_opener() {
            let url = url.to_owned();
            self.foreground_executor
                .spawn(async move {
                    if let Err(error) = opener.open_link(url.clone()).await {
                        log::warn!("could not open {url}: {error:?}");
                    }
                })
                .detach();
        }
    }

    /// WaOS's open requests are not delivered yet.
    fn on_open_urls(&self, _callback: Box<dyn FnMut(Vec<String>)>) {}

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("WaOS registers no URL schemes for a program")))
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        file_dialogs::prompt_for_paths(&self.foreground_executor, options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        file_dialogs::prompt_for_new_path(&self.foreground_executor, directory, suggested_name)
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, path: &Path) {
        if let Some(opener) = wasdk::opener::get_opener() {
            let path = path.to_string_lossy().into_owned();
            self.foreground_executor
                .spawn(async move {
                    let _ = opener.reveal_file(path).await;
                })
                .detach();
        }
    }

    fn open_with_system(&self, path: &Path) {
        if let Some(opener) = wasdk::opener::get_opener() {
            let path = path.to_string_lossy().into_owned();
            self.foreground_executor
                .spawn(async move {
                    let _ = opener.open_file(path).await;
                })
                .detach();
        }
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.callbacks.quit.set(callback);
    }

    fn on_reopen(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn set_menus(&self, menus: Vec<Menu>, _keymap: &Keymap) {
        *self.menus.borrow_mut() = menus.into_iter().map(|menu| menu.owned()).collect();
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        Some(self.menus.borrow().clone())
    }

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    /// No menu bar is set yet, so no menu action comes.
    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }

    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}

    fn prevent_idle_sleep(&self, _reason: &str) -> Task<Result<ActivityGuard>> {
        Task::ready(Ok(ActivityGuard::noop()))
    }

    fn compositor_name(&self) -> &'static str {
        "WaOS"
    }

    fn app_path(&self) -> Result<PathBuf> {
        Err(anyhow!("a WaOS program has no path of its own"))
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow!("a WaOS program has no executables beside it"))
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        let cursor = cursor_of(style);
        if self.cursor.replace(cursor) != cursor && !self.cursor_hidden.get() {
            self.apply_cursor(cursor);
        }
    }

    fn hide_cursor_until_mouse_moves(&self) {
        if !self.cursor_hidden.replace(true) {
            self.apply_cursor(Cursor::None);
        }
    }

    fn is_cursor_visible(&self) -> bool {
        !self.cursor_hidden.get()
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        clipboard::get_clipboard().and_then(|clipboard| clipboard_text::read_blocking(&clipboard))
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        if let (Some(clipboard), Some(text)) = (clipboard::get_clipboard(), item.text()) {
            clipboard_text::write(&self.foreground_executor, clipboard, text);
        }
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("WaOS keeps no credentials for a program")))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(UnknownLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}

/// WaOS does not say the keyboard's layout; keys come already meant under it (`logical`).
struct UnknownLayout;

impl PlatformKeyboardLayout for UnknownLayout {
    fn id(&self) -> &str {
        "unknown"
    }

    fn name(&self) -> &str {
        "Unknown"
    }
}

fn window_config(params: &WindowParams) -> WindowConfig {
    let config = WindowConfig::new();
    if let Some(title) = params
        .titlebar
        .as_ref()
        .and_then(|titlebar| titlebar.title.as_ref())
    {
        config.set_title(title);
    }
    let bounds = params.bounds;
    config.set_size(WindowSize {
        width: f64::from(f32::from(bounds.size.width)),
        height: f64::from(f32::from(bounds.size.height)),
    });
    config.set_position(WindowPoint {
        x: f64::from(f32::from(bounds.origin.x)),
        y: f64::from(f32::from(bounds.origin.y)),
    });
    config.set_resizable(params.is_resizable);
    config
}

fn appearance_of(brightness: Brightness) -> WindowAppearance {
    match brightness {
        Brightness::Light => WindowAppearance::Light,
        Brightness::Dark => WindowAppearance::Dark,
    }
}

fn operating_system() -> OperatingSystem {
    match system::os() {
        system::OperatingSystem::Macos => OperatingSystem::Mac,
        system::OperatingSystem::Windows => OperatingSystem::Windows,
        system::OperatingSystem::Ios => OperatingSystem::Ios,
        system::OperatingSystem::Android => OperatingSystem::Android,
        system::OperatingSystem::Linux
        | system::OperatingSystem::Fuchsia
        | system::OperatingSystem::Other => OperatingSystem::Linux,
    }
}
