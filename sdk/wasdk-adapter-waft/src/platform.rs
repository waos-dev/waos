//! The platform as Waft sees it: the program's views, the screens, what the user set, the
//! clipboard, the menus, the file dialogs, the opener, the notifications, the players, the reading
//! of media, the image codecs, the frame readers and the cameras, the compositor its composition layers are made on, and the frame requests, cursors, focus requests,
//! view requests and wake-ups it passes on.
//!
//! Waft's model and the WIT's map one to one (the design's decision 4): the views the
//! shell gave are [`Platform::views`] when the app starts, a view that arrives later is the
//! client's `view_added`, and [`Platform::request_view`] is `request-view`, whose id the view
//! carries when it arrives. A view keeps the engine's id for it.
//!
//! Frames are per view, one to one: [`Platform::request_frame`] of a view is that view's
//! `request-frame`, and the loop gives the frame that answers it to the client as that view's. A
//! request for a view the program no longer has asks nothing.
//!
//! Whether the framework handles Back is the app's, and the WIT's is each view's: what the
//! framework said last is said on every view the program has, and on each that arrives after, so
//! a Back on any of them reaches the app's navigators while they can pop.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use futures::channel::mpsc;
use waft_embedder::image_codecs::ImageCodecs;
use waft_embedder::{
    AppExit, AppLifecycleState, Brightness, Cameras, CaptionSettings, Clipboard, Compositor,
    Dispatcher, FileDialogFuture, FileDialogs, FolderOptions, FontSource, HardwareBuffer,
    HardwareBufferAllocator, ImageCodecFuture, ImageDecodeError, Instant, Locale, MediaAssets, MenuBar, MenuEntry, Notifications,
    Offset, OpenOptions, Opener, PickedFile, PickedFolder, Platform, Players, PopupMenuFuture,
    PopupMenus, RequestId, SaveOptions, Screens, SignedDuration, TargetPlatform, TrayIcons,
    VideoFrame, VideoFrameReaders, ViewError, ViewId, ViewRef, WindowBackground, WindowConfig,
};
use wasdk::{lifecycle, system, views};

use crate::app_exit::WasdkAppExit;
use crate::cameras::WasdkCameras;
use crate::caption_settings::WasdkCaptionSettings;
use crate::clipboard::WasdkClipboard;
use crate::composition::WasdkCompositor;
use crate::dispatcher::Wake;
use crate::events::{Notice, StreamOpener};
use crate::file_dialogs::WasdkFileDialogs;
use crate::fonts::ProgramFonts;
use crate::gpu_interop;
use crate::hardware_buffers::WasdkHardwareBufferAllocator;
use crate::image_codecs::WasdkImageCodecs;
use crate::image_decoding::ImageDecoding;
use crate::lifecycle_states::framework_lifecycle_state;
use crate::media_assets::WasdkMediaAssets;
use crate::menus::{WasdkContextMenus, WasdkMenuBar, WasdkTrayIcons};
use crate::notifications::WasdkNotifications;
use crate::opener::WasdkOpener;
use crate::players::WasdkPlayers;
use crate::renderer::Renderer;
use crate::screens::WasdkScreens;
use crate::system::{brightness_of, instant_of, locale_of, signed_duration_of, target_platform_of};
use crate::text_input::WasdkTextInput;
use crate::video_frames::WasdkVideoFrameReaders;
use crate::views::WasdkView;
use crate::windows::{WasdkWindow, clear_of, view_error, window_config};

/// One view of the program and, where it is in one, its window.
struct ViewWithWindow {
    view: Rc<WasdkView>,
    /// Held for as long as the view is the program's, which reaches it weakly as its window.
    _window: Option<Rc<WasdkWindow>>,
}

pub(crate) struct WasdkPlatform {
    target_platform: TargetPlatform,
    brightness: Cell<Brightness>,
    locales: RefCell<Vec<Locale>>,
    screens: WasdkScreens,
    /// The platform's clipboard, where it has one.
    clipboard: Option<WasdkClipboard>,
    /// The platform's menu bar, context menus and tray, each where it has it.
    menu_bar: Option<WasdkMenuBar>,
    context_menus: Option<WasdkContextMenus>,
    tray_icons: Option<WasdkTrayIcons>,
    /// The platform's file dialogs, where it has them.
    file_dialogs: Option<WasdkFileDialogs>,
    /// The system's opener and its notifications, each where the platform has it.
    opener: Option<WasdkOpener>,
    notifications: Option<WasdkNotifications>,
    /// The system's players, reading of media, image codecs, frame readers and cameras, each where
    /// it has them.
    players: Option<WasdkPlayers>,
    media_assets: Option<WasdkMediaAssets>,
    image_codecs: Option<WasdkImageCodecs>,
    /// What Waft's own images decode with, over the image codecs, where the system has them.
    image_decoding: Option<ImageDecoding>,
    video_frame_readers: Option<WasdkVideoFrameReaders>,
    cameras: Option<WasdkCameras>,
    /// The user's caption settings, where the system keeps them.
    caption_settings: Option<WasdkCaptionSettings>,
    /// The system's compositor, where it has one, which the app's composition layers are made on.
    compositor: Option<Arc<WasdkCompositor>>,
    /// The system's hardware buffer allocator, where it allocates buffers.
    hardware_buffer_allocator: Option<Arc<WasdkHardwareBufferAllocator>>,
    /// The program's views, by the engine's id for each, which grows as they arrive.
    views: RefCell<BTreeMap<ViewId, ViewWithWindow>>,
    /// The background of each window asked for whose view has not arrived, for what its frames
    /// start from.
    requested_backgrounds: RefCell<HashMap<RequestId, WindowBackground>>,
    renderer: Rc<Renderer>,
    /// Where the framework asks for its turns; the loop waits until the next.
    wake: Arc<Wake>,
    /// Where the app asks to end itself.
    app_exit: WasdkAppExit,
}

impl WasdkPlatform {
    /// The platform as `system` and `screens` answer now, with no view yet, drawing with
    /// `renderer`; the framework's wake-ups and quit go to `notices`, and the streams of the tray
    /// icons the app adds to `opener`.
    pub(crate) fn new(
        renderer: Rc<Renderer>,
        notices: mpsc::UnboundedSender<Notice>,
        opener: StreamOpener,
    ) -> WasdkPlatform {
        let image_codecs = WasdkImageCodecs::get();
        WasdkPlatform {
            target_platform: target_platform_of(system::os()),
            brightness: Cell::new(brightness_of(system::platform_brightness())),
            locales: RefCell::new(system::locales().into_iter().map(locale_of).collect()),
            screens: WasdkScreens::new(),
            clipboard: WasdkClipboard::get(),
            menu_bar: WasdkMenuBar::get(),
            context_menus: WasdkContextMenus::get(),
            tray_icons: WasdkTrayIcons::get(opener),
            file_dialogs: WasdkFileDialogs::get(),
            opener: WasdkOpener::get(),
            compositor: WasdkCompositor::get().map(Arc::new),
            hardware_buffer_allocator: WasdkHardwareBufferAllocator::get().map(Arc::new),
            notifications: WasdkNotifications::get(),
            players: WasdkPlayers::get(),
            media_assets: WasdkMediaAssets::get(),
            image_decoding: image_codecs
                .as_ref()
                .map(|codecs| ImageDecoding::new(codecs, Rc::clone(&renderer))),
            image_codecs,
            video_frame_readers: WasdkVideoFrameReaders::get(),
            cameras: WasdkCameras::get(),
            caption_settings: WasdkCaptionSettings::get(),
            views: RefCell::default(),
            requested_backgrounds: RefCell::default(),
            renderer,
            wake: Arc::new(Wake::new(notices.clone())),
            app_exit: WasdkAppExit::new(notices),
        }
    }

    pub(crate) fn wake(&self) -> &Arc<Wake> {
        &self.wake
    }

    /// Makes `view` one of the program's, in its window where it has one, and answers the
    /// framework's view of it; none when the program has it already, as a view given at the
    /// start and reported again on `view-changes` is.
    pub(crate) fn add_view(&self, view: views::View) -> Option<Rc<WasdkView>> {
        let id = ViewId(view.id());
        if self.views.borrow().contains_key(&id) {
            return None;
        }
        let window = view.window();
        let clear = self.clear_for(view.request_id().map(RequestId));
        // The host sends Edit's commands only where the program has a menu bar, with Edit's.
        let host_edit_commands = self.menu_bar.is_some();
        let view = Rc::new(WasdkView::new(
            view,
            Rc::clone(&self.renderer),
            clear,
            host_edit_commands,
        ));
        let window = window.map(|window| WasdkWindow::around(window, &view));
        self.views.borrow_mut().insert(
            id,
            ViewWithWindow {
                view: Rc::clone(&view),
                _window: window,
            },
        );
        Some(view)
    }

    /// Takes the view away, answering whether it was still there.
    pub(crate) fn remove_view(&self, id: ViewId) -> bool {
        self.views.borrow_mut().remove(&id).is_some()
    }

    /// Whether the program still has the view `id`.
    pub(crate) fn has_view(&self, id: ViewId) -> bool {
        self.views.borrow().contains_key(&id)
    }

    /// The request `id` got no view: its window's background is forgotten.
    pub(crate) fn request_failed(&self, id: RequestId) {
        self.requested_backgrounds.borrow_mut().remove(&id);
    }

    /// The text input of the view `id`, where it is still there and has one.
    pub(crate) fn text_input(&self, id: ViewId) -> Option<Rc<WasdkTextInput>> {
        self.wasdk_view(id)?.wasdk_text_input()
    }

    pub(crate) fn set_metrics(&self, id: ViewId, metrics: views::Metrics) {
        if let Some(view) = self.wasdk_view(id) {
            view.set_metrics(metrics);
        }
    }

    pub(crate) fn set_brightness(&self, brightness: system::Brightness) {
        self.brightness.set(brightness_of(brightness));
    }

    pub(crate) fn set_locales(&self, locales: Vec<system::Locale>) {
        *self.locales.borrow_mut() = locales.into_iter().map(locale_of).collect();
    }

    /// The screens changed: they are read again.
    pub(crate) fn refresh_screens(&self) {
        self.screens.refresh();
    }

    /// Asks for every form the app's copy on the clipboard still promises, files aside, so what
    /// it gives outlives the program; resolves at once where the platform has no clipboard.
    pub(crate) async fn flush_clipboard(&self) {
        if let Some(clipboard) = &self.clipboard {
            clipboard.flush().await;
        }
    }

    pub(crate) fn wasdk_view(&self, id: ViewId) -> Option<Rc<WasdkView>> {
        let views = self.views.borrow();
        views.get(&id).map(|entry| Rc::clone(&entry.view))
    }

    /// What the frames of the view arriving for `request` start from: as its window's background
    /// says, and white for a view the shell gave.
    fn clear_for(&self, request: Option<RequestId>) -> valo::Color {
        let background = request
            .and_then(|request| self.requested_backgrounds.borrow_mut().remove(&request))
            .unwrap_or_default();
        clear_of(background)
    }
}

impl Platform for WasdkPlatform {
    fn target_platform(&self) -> TargetPlatform {
        self.target_platform
    }

    /// `request-frame` of the view: its frame comes within the refresh its screen is in, or at
    /// the next where it had that one's, while it is on screen, and the loop gives it to the
    /// client as that view's.
    fn request_frame(&self, view: ViewId) {
        if let Some(view) = self.wasdk_view(view) {
            view.request_framework_frame();
        }
    }

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn dispatcher(&self) -> Arc<dyn Dispatcher> {
        Arc::clone(&self.wake) as Arc<dyn Dispatcher>
    }

    fn views(&self) -> Vec<ViewRef> {
        self.views
            .borrow()
            .values()
            .map(|entry| Rc::clone(&entry.view) as ViewRef)
            .collect()
    }

    fn view(&self, id: ViewId) -> Option<ViewRef> {
        self.wasdk_view(id).map(|view| view as ViewRef)
    }

    /// `request-view`; the view arrives on `view-changes` carrying the id answered here.
    fn request_view(
        &self,
        config: WindowConfig,
        initial_data: Option<String>,
    ) -> Result<RequestId, ViewError> {
        let id = views::request_view(&window_config(&config), initial_data.as_deref())
            .map(RequestId)
            .map_err(view_error)?;
        self.requested_backgrounds
            .borrow_mut()
            .insert(id, config.background);
        Ok(id)
    }

    fn locales(&self) -> Vec<Locale> {
        self.locales.borrow().clone()
    }

    fn platform_brightness(&self) -> Brightness {
        self.brightness.get()
    }

    /// `utc-offset` at that instant, as Waft's duration; UTC's zero, Waft's default, where the
    /// system has no time zone.
    fn time_zone_offset(&self, seconds_since_epoch: i64) -> SignedDuration {
        system::utc_offset(instant_of(seconds_since_epoch))
            .map_or(SignedDuration::ZERO, signed_duration_of)
    }

    /// `time-zone-abbreviation` at that instant; Waft's default "UTC" where the system has no
    /// time zone.
    fn time_zone_name(&self, seconds_since_epoch: i64) -> String {
        system::time_zone_abbreviation(instant_of(seconds_since_epoch))
            .unwrap_or_else(|| "UTC".to_owned())
    }

    /// The state the program is in when the framework first asks, which is when its binding is
    /// made; each change after reaches it from the loop, whose stream was opened before.
    fn initial_lifecycle_state(&self) -> Option<AppLifecycleState> {
        Some(framework_lifecycle_state(lifecycle::state()))
    }

    fn font_source(&self) -> Option<Box<dyn FontSource>> {
        Some(Box::new(ProgramFonts::new()))
    }

    /// Those of the system the engine runs on, the one the widgets follow, as native Waft's are
    /// there: a component is built for no system of its own.
    fn default_font_families(&self) -> Vec<String> {
        waft_embedder::default_font_families(self.target_platform)
    }

    /// Through the system's image codecs (`image_decoding`); `NoDecoder` where it has none.
    fn open_image_codec(&self, bytes: Arc<[u8]>) -> ImageCodecFuture {
        match &self.image_decoding {
            Some(decoding) => decoding.open_codec(bytes),
            None => Box::pin(std::future::ready(Err(ImageDecodeError::NoDecoder))),
        }
    }

    fn import_pixels(&self, pixels: valo::PixelBuffer) -> Option<waft_embedder::Image> {
        self.renderer.import_pixels(pixels, false)
    }

    fn import_video_frame(&self, frame: &dyn VideoFrame) -> Option<waft_embedder::Image> {
        self.renderer.import_frame(frame)
    }

    fn import_hardware_buffer(&self, buffer: &dyn HardwareBuffer) -> Option<waft_embedder::Image> {
        self.renderer.import_buffer(buffer, false)
    }

    /// The WIT's `import-texture` on `device`, for a buffer the WIT handed out
    /// (`gpu_interop`).
    fn import_texture(
        &self,
        buffer: &dyn HardwareBuffer,
        device: &wgpu::Device,
    ) -> Option<wgpu::Texture> {
        gpu_interop::import_texture(buffer, device)
    }

    fn hardware_buffer_allocator(&self) -> Option<Arc<dyn HardwareBufferAllocator>> {
        self.hardware_buffer_allocator
            .clone()
            .map(|allocator| allocator as Arc<dyn HardwareBufferAllocator>)
    }

    fn compositor(&self) -> Option<Arc<dyn Compositor>> {
        self.compositor
            .clone()
            .map(|compositor| compositor as Arc<dyn Compositor>)
    }

    fn screens(&self) -> Option<&dyn Screens> {
        Some(&self.screens)
    }

    fn clipboard(&self) -> Option<&dyn Clipboard> {
        self.clipboard
            .as_ref()
            .map(|clipboard| clipboard as &dyn Clipboard)
    }

    fn menu_bar(&self) -> Option<&dyn MenuBar> {
        self.menu_bar
            .as_ref()
            .map(|menu_bar| menu_bar as &dyn MenuBar)
    }

    fn popup_menus(&self) -> Option<&dyn PopupMenus> {
        self.context_menus.as_ref().map(|_| self as &dyn PopupMenus)
    }

    fn tray_icons(&self) -> Option<&dyn TrayIcons> {
        self.tray_icons
            .as_ref()
            .map(|tray_icons| tray_icons as &dyn TrayIcons)
    }

    fn file_dialogs(&self) -> Option<&dyn FileDialogs> {
        self.file_dialogs.as_ref().map(|_| self as &dyn FileDialogs)
    }

    fn opener(&self) -> Option<&dyn Opener> {
        self.opener.as_ref().map(|opener| opener as &dyn Opener)
    }

    fn notifications(&self) -> Option<&dyn Notifications> {
        self.notifications
            .as_ref()
            .map(|notifications| notifications as &dyn Notifications)
    }

    fn players(&self) -> Option<&dyn Players> {
        self.players.as_ref().map(|players| players as &dyn Players)
    }

    fn media_assets(&self) -> Option<&dyn MediaAssets> {
        self.media_assets
            .as_ref()
            .map(|assets| assets as &dyn MediaAssets)
    }

    fn image_codecs(&self) -> Option<&dyn ImageCodecs> {
        self.image_codecs
            .as_ref()
            .map(|codecs| codecs as &dyn ImageCodecs)
    }

    fn video_frame_readers(&self) -> Option<&dyn VideoFrameReaders> {
        self.video_frame_readers
            .as_ref()
            .map(|readers| readers as &dyn VideoFrameReaders)
    }

    fn cameras(&self) -> Option<&dyn Cameras> {
        self.cameras.as_ref().map(|cameras| cameras as &dyn Cameras)
    }

    fn caption_settings(&self) -> Option<&dyn CaptionSettings> {
        self.caption_settings
            .as_ref()
            .map(|settings| settings as &dyn CaptionSettings)
    }

    /// Every program can end itself, by returning from `main`.
    fn app_exit(&self) -> Option<&dyn AppExit> {
        Some(&self.app_exit)
    }
}

impl PopupMenus for WasdkPlatform {
    /// The menu is the view's: none is opened for a view the program no longer has.
    fn show(&self, view: ViewId, at: Offset, menu: &[MenuEntry]) -> PopupMenuFuture {
        match (&self.context_menus, self.wasdk_view(view)) {
            (Some(menus), Some(view)) => menus.show(view, at, menu),
            _ => Box::pin(std::future::ready(None)),
        }
    }
}

/// Each dialog is a sheet on the window of the view `parent`, while the program has it.
impl FileDialogs for WasdkPlatform {
    fn open_file(
        &self,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PickedFile>> {
        self.dialogs().open_file(self.parent(parent), options)
    }

    fn open_files(
        &self,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PickedFile>> {
        self.dialogs().open_files(self.parent(parent), options)
    }

    fn open_folder(
        &self,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PickedFolder>> {
        self.dialogs().open_folder(self.parent(parent), options)
    }

    fn save_file(
        &self,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PickedFile>> {
        self.dialogs().save_file(self.parent(parent), options)
    }
}

impl WasdkPlatform {
    /// The file dialogs, which a platform answering `file_dialogs` has.
    fn dialogs(&self) -> &WasdkFileDialogs {
        self.file_dialogs
            .as_ref()
            .expect("asked only where `file_dialogs` answered some")
    }

    /// The view a dialog is a sheet on's window, while the program has it.
    fn parent(&self, parent: Option<ViewId>) -> Option<Rc<WasdkView>> {
        self.wasdk_view(parent?)
    }
}
