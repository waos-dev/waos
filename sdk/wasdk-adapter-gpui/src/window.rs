//! A GPUI window over a wasdk view.
//!
//! GPUI opens a window synchronously, but a program asks WaOS for a view and the view arrives
//! later, once the system's window is on screen. So a window opens at once, pending: it answers
//! with the bounds GPUI asked for and the primary screen's scale, hands out its atlas, and draws
//! nothing. When its view arrives ([`WindowShared::attach`]), it makes its renderer on the view's
//! surface, tells GPUI its real size, and draws from its first frame.
//!
//! The view's metrics say where the platform's own UI covers it: the status bar, a notch, the
//! navigation bar, and the keyboard. The window gives them to GPUI as its insets, its safe area and
//! its keyboard, which GPUI leaves the app to keep clear of.
//!
//! Frames are on demand, wasdk's model and GPUI's: GPUI wakes the window when it is dirty, which
//! asks the view for a frame, and the frame that arrives runs GPUI's frame callback, which draws.
//!
//! The invariant: GPUI's callbacks re-enter the window (drawing, setting the input handler,
//! asking for frames), so none is called with a borrow of the window's state held; each is lent
//! out of its [`Slot`] for the call.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gpui::{
    AnyWindowHandle, Bounds, Capslock, DevicePixels, DispatchEventResult, Edges,
    FrameRequestSource, GpuSpecs, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    ModifiersChangedEvent, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput,
    PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions,
    Scene, Size, TextInputConfiguration, WindowAppearance, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowInsets, WindowParams, WindowVisibility, px, size,
};
use gpui_wgpu::{WgpuAtlas, WgpuContext, WgpuRenderer, WgpuSurfaceConfig};
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use wasdk::views::{
    Insets, KeyInput, KeyKind, Metrics, TextInputAction, View, ViewEvent, ViewFocusState,
};
use wasdk::windows::Window as SystemWindow;

use crate::display::bounds_of;
use crate::keys::{HeldModifiers, KeyMeaning};
use crate::pointer::{Hover, PointerState};
use crate::slot::Slot;
use crate::text_input::{ENTER, TextInputBridge, Typing};

/// The window GPUI holds.
pub(crate) struct WasdkWindow {
    shared: Rc<WindowShared>,
}

/// What the window and the event loop share.
pub(crate) struct WindowShared {
    pub(crate) handle: AnyWindowHandle,
    context: WgpuContext,
    atlas: Arc<WgpuAtlas>,
    display: Rc<dyn PlatformDisplay>,
    appearance: Rc<Cell<WindowAppearance>>,
    view: RefCell<Option<AttachedView>>,
    state: RefCell<WindowState>,
    callbacks: Callbacks,
    /// A frame was asked of the view and has not arrived.
    frame_requested: Cell<bool>,
    /// GPUI wants a frame: asked of the view as soon as it arrives.
    frame_wanted: Cell<bool>,
    /// The next frame redraws everything: the window's size or scale changed.
    force_render: Cell<bool>,
}

struct AttachedView {
    view: Arc<View>,
    system_window: Option<SystemWindow>,
    renderer: RefCell<WgpuRenderer>,
    text_input: Option<RefCell<TextInputBridge>>,
}

struct WindowState {
    /// The window's content, in logical pixels, at its place on its screen.
    bounds: Bounds<Pixels>,
    scale: f32,
    /// Where the platform's UI covers the window, in logical pixels.
    insets: WindowInsets,
    title: String,
    active: bool,
    hovered: bool,
    pointer: PointerState,
    keys: HeldModifiers,
    input_handler: Option<PlatformInputHandler>,
    /// What the focused input asks of the input method, as GPUI said it last.
    text_input_configuration: TextInputConfiguration,
}

type ResizeCallback = Box<dyn FnMut(Size<Pixels>, f32)>;
type InputCallback = Box<dyn FnMut(PlatformInput) -> DispatchEventResult>;

#[derive(Default)]
struct Callbacks {
    request_frame: Slot<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Slot<InputCallback>,
    active_status_change: Slot<Box<dyn FnMut(bool)>>,
    hover_status_change: Slot<Box<dyn FnMut(bool)>>,
    resize: Slot<ResizeCallback>,
    insets_changed: Slot<Box<dyn FnMut(WindowInsets)>>,
    should_close: Slot<Box<dyn FnMut() -> bool>>,
    close: Slot<Box<dyn FnOnce()>>,
    appearance_changed: Slot<Box<dyn FnMut()>>,
}

impl WasdkWindow {
    /// A window opened as GPUI asks, pending until its view arrives.
    pub(crate) fn open(
        handle: AnyWindowHandle,
        params: &WindowParams,
        context: WgpuContext,
        display: Rc<dyn PlatformDisplay>,
        scale: f32,
        appearance: Rc<Cell<WindowAppearance>>,
    ) -> WasdkWindow {
        let title = params
            .titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.as_ref())
            .map(|title| title.to_string())
            .unwrap_or_default();
        // Not `Send` on wasm, where wgpu's objects are not; GPUI asks for an `Arc` regardless, as
        // `gpui_wgpu`'s own renderer makes one.
        #[allow(clippy::arc_with_non_send_sync)]
        let atlas = Arc::new(WgpuAtlas::from_context(&context));
        let shared = Rc::new(WindowShared {
            handle,
            context,
            atlas,
            display,
            appearance,
            view: RefCell::new(None),
            state: RefCell::new(WindowState {
                bounds: params.bounds,
                scale,
                insets: WindowInsets::default(),
                title,
                active: false,
                hovered: false,
                pointer: PointerState::default(),
                keys: HeldModifiers::default(),
                input_handler: None,
                text_input_configuration: TextInputConfiguration::default(),
            }),
            callbacks: Callbacks::default(),
            frame_requested: Cell::new(false),
            frame_wanted: Cell::new(false),
            force_render: Cell::new(true),
        });
        WasdkWindow { shared }
    }

    pub(crate) fn shared(&self) -> Rc<WindowShared> {
        Rc::clone(&self.shared)
    }
}

impl WindowShared {
    /// The view WaOS gave this window: from now on the window draws on it.
    pub(crate) fn attach(&self, view: View) -> anyhow::Result<()> {
        let system_window = view.window();
        let text_input = view
            .text_input()
            .map(|input| RefCell::new(TextInputBridge::new(input)));
        let view = Arc::new(view);
        let metrics = view.metrics();
        let surface = self
            .context
            .instance
            .create_surface(view.clone())
            .map_err(|error| anyhow::anyhow!("no surface on the view: {error}"))?;
        let renderer = WgpuRenderer::new_from_surface_with_atlas(
            &self.context,
            surface,
            WgpuSurfaceConfig {
                size: size(
                    DevicePixels(metrics.width as i32),
                    DevicePixels(metrics.height as i32),
                ),
                transparent: false,
                preferred_present_mode: None,
            },
            Arc::clone(&self.atlas),
        )?;
        if let Some(window) = &system_window {
            let title = self.state.borrow().title.clone();
            if !title.is_empty() {
                window.set_title(&title);
            }
            let origin = bounds_of(window.frame()).origin;
            self.state.borrow_mut().bounds.origin = origin;
        }
        if let Some(style) = crate::status_bar::said() {
            view.set_overlay_style(style);
        }
        *self.view.borrow_mut() = Some(AttachedView {
            view,
            system_window,
            renderer: RefCell::new(renderer),
            text_input,
        });
        self.resized(&metrics);
        self.request_frame();
        Ok(())
    }

    /// Shows the system's bars over the window in `style`, once its view has arrived.
    pub(crate) fn set_overlay_style(&self, style: wasdk::views::OverlayStyle) {
        if let Some(attached) = self.view.borrow().as_ref() {
            attached.view.set_overlay_style(style);
        }
    }

    /// One event of the window's view.
    pub(crate) fn view_event(&self, event: ViewEvent) {
        match event {
            ViewEvent::Frame(_) => self.frame_arrived(),
            ViewEvent::Metrics(metrics) => self.resized(&metrics),
            ViewEvent::Pointer(pointer) => self.pointer_event(&pointer),
            ViewEvent::Key(key) => self.key_event(key),
            ViewEvent::Focus(focus) => self.active_changed(focus.state == ViewFocusState::Focused),
            ViewEvent::TextInputEditingValue(value) => {
                let mut typing = Vec::new();
                self.with_text_input(|bridge, handler| {
                    if let Some(handler) = handler {
                        typing = bridge.apply(value, handler);
                    }
                });
                for step in typing {
                    self.type_step(step);
                }
            }
            ViewEvent::TextInputClosed => {
                self.with_text_input(|bridge, handler| bridge.closed(handler))
            }
            ViewEvent::CloseRequested => self.close_requested(),
            // The action on the keyboard's Enter key is how the key looks: pressing it is Enter,
            // as GPUI has it. A new line arrives as an edit of the value instead.
            ViewEvent::TextInputAction(TextInputAction::Newline) => {}
            ViewEvent::TextInputAction(_) => self.type_step(Typing::Key(ENTER)),
            ViewEvent::Copy(_)
            | ViewEvent::Cut(_)
            | ViewEvent::Paste(_)
            | ViewEvent::Edit(_)
            | ViewEvent::PopRoute => {}
        }
    }

    /// The view is gone: the window closed.
    pub(crate) fn removed(&self) {
        self.view.borrow_mut().take();
        if let Some(close) = self.callbacks.close.take() {
            close();
        }
    }

    pub(crate) fn appearance_changed(&self) {
        self.callbacks
            .appearance_changed
            .call(|callback| callback());
    }

    /// GPUI's input handler and the input method agree: called after each frame, when GPUI has
    /// set the focused input's handler.
    pub(crate) fn sync_text_input(&self) {
        let (scale, configuration) = {
            let state = self.state.borrow();
            (state.scale, state.text_input_configuration.clone())
        };
        self.with_text_input(|bridge, handler| bridge.sync(handler, &configuration, scale));
    }

    fn frame_arrived(&self) {
        self.frame_requested.set(false);
        let force_render = self.force_render.replace(false);
        self.callbacks.request_frame.call(|callback| {
            callback(RequestFrameOptions {
                require_presentation: false,
                force_render,
                signal_at: None,
                signal_source: FrameRequestSource::NativeCallback,
            })
        });
        self.sync_text_input();
    }

    /// The view's size, scale and insets are `metrics`: GPUI hears of the size and scale, and of
    /// the insets where they changed.
    fn resized(&self, metrics: &Metrics) {
        let (width, height) = (metrics.width, metrics.height);
        let scale = metrics.device_pixel_ratio as f32;
        let logical = size(px(width as f32 / scale), px(height as f32 / scale));
        let insets = insets_of(metrics, scale);
        let insets_changed = {
            let mut state = self.state.borrow_mut();
            state.bounds.size = logical;
            state.scale = scale;
            let changed = state.insets != insets;
            state.insets = insets.clone();
            changed
        };
        if let Some(attached) = self.view.borrow().as_ref() {
            attached.renderer.borrow_mut().update_drawable_size(size(
                DevicePixels(width as i32),
                DevicePixels(height as i32),
            ));
        }
        self.force_render.set(true);
        self.callbacks
            .resize
            .call(|callback| callback(logical, scale));
        if insets_changed {
            self.callbacks
                .insets_changed
                .call(|callback| callback(insets));
        }
        self.request_frame();
    }

    fn pointer_event(&self, event: &wasdk::views::PointerEvent) {
        let (input, hover) = {
            let mut state = self.state.borrow_mut();
            let scale = state.scale;
            let modifiers = state.keys.modifiers();
            state.pointer.input(event, scale, modifiers)
        };
        match hover {
            Hover::Entered => self.hover_changed(true),
            Hover::Left => self.hover_changed(false),
            Hover::Unchanged => {}
        }
        if let Some(input) = input {
            self.dispatch(input);
        }
    }

    fn hover_changed(&self, hovered: bool) {
        self.state.borrow_mut().hovered = hovered;
        self.callbacks
            .hover_status_change
            .call(|callback| callback(hovered));
    }

    /// A key, as GPUI's keystroke; one GPUI leaves that types text is typed into the focused
    /// input, as the platforms' text input does. The reply says whether either took it, so one
    /// neither took goes on to the platform's menus.
    fn key_event(&self, KeyInput { event, reply }: KeyInput) {
        let meaning = self.state.borrow_mut().keys.meaning(&event);
        let handled = match meaning {
            KeyMeaning::ModifiersChanged(modifiers, capslock) => {
                self.dispatch(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                    modifiers,
                    capslock,
                }));
                false
            }
            KeyMeaning::Keystroke {
                keystroke,
                kind: KeyKind::Up,
            } => {
                let result = self.dispatch(PlatformInput::KeyUp(KeyUpEvent { keystroke }));
                result.is_some_and(|result| !result.propagate)
            }
            KeyMeaning::Keystroke { keystroke, kind } => {
                let key_char = keystroke.key_char.clone();
                let result = self.dispatch(PlatformInput::KeyDown(KeyDownEvent {
                    keystroke,
                    is_held: kind == KeyKind::Repeat,
                    prefer_character_input: false,
                }));
                let taken = result.is_some_and(|result| !result.propagate);
                taken || key_char.is_some_and(|text| self.type_text(&text))
            }
            KeyMeaning::Unnamed => false,
        };
        reply.handled(handled);
    }

    /// Does `step` of an edit of an input with no document: types or shows its text in the
    /// focused input, or presses the key it means, as a keyboard's key reaches GPUI.
    fn type_step(&self, step: Typing) {
        match step {
            Typing::Text(text) => {
                self.type_text(&text);
            }
            Typing::Preview(text) => {
                self.with_input_handler(|handler| {
                    handler.replace_and_mark_text_in_range(None, &text, None)
                });
            }
            Typing::EndPreview => {
                self.with_input_handler(|handler| handler.unmark_text());
            }
            Typing::Key(key) => {
                let keystroke = Keystroke {
                    modifiers: Modifiers::default(),
                    key: key.to_owned(),
                    key_char: None,
                };
                self.dispatch(PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }));
                self.dispatch(PlatformInput::KeyUp(KeyUpEvent { keystroke }));
            }
        }
    }

    /// Types `text` into the focused input; whether there was one.
    fn type_text(&self, text: &str) -> bool {
        self.with_input_handler(|handler| handler.replace_text_in_range(None, text))
            .is_some()
    }

    fn active_changed(&self, active: bool) {
        self.state.borrow_mut().active = active;
        self.callbacks
            .active_status_change
            .call(|callback| callback(active));
    }

    fn close_requested(&self) {
        let close = self
            .callbacks
            .should_close
            .call(|callback| callback())
            .unwrap_or(true);
        if close {
            self.close_system_window();
        }
    }

    fn close_system_window(&self) {
        if let Some(window) = self
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.close();
        }
    }

    fn dispatch(&self, input: PlatformInput) -> Option<DispatchEventResult> {
        self.callbacks.input.call(|callback| callback(input))
    }

    /// Asks the view for a frame, once until it arrives; or as soon as there is a view.
    fn request_frame(&self) {
        let view = self.view.borrow();
        let Some(attached) = view.as_ref() else {
            self.frame_wanted.set(true);
            return;
        };
        if !self.frame_requested.replace(true) {
            attached.view.request_frame();
        }
    }

    fn with_input_handler<R>(&self, f: impl FnOnce(&mut PlatformInputHandler) -> R) -> Option<R> {
        let mut handler = self.state.borrow_mut().input_handler.take()?;
        let result = f(&mut handler);
        let mut state = self.state.borrow_mut();
        if state.input_handler.is_none() {
            state.input_handler = Some(handler);
        }
        Some(result)
    }

    fn with_text_input(
        &self,
        f: impl FnOnce(&mut TextInputBridge, Option<&mut PlatformInputHandler>),
    ) {
        let view = self.view.borrow();
        let Some(bridge) = view
            .as_ref()
            .and_then(|attached| attached.text_input.as_ref())
        else {
            return;
        };
        let mut bridge = bridge.borrow_mut();
        let mut handler = self.state.borrow_mut().input_handler.take();
        f(&mut bridge, handler.as_mut());
        let mut state = self.state.borrow_mut();
        if state.input_handler.is_none() {
            state.input_handler = handler;
        }
    }
}

impl Drop for WasdkWindow {
    /// GPUI let the window go: the system's window closes with it.
    fn drop(&mut self) {
        self.shared.close_system_window();
    }
}

impl HasWindowHandle for WasdkWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, HandleError> {
        Err(HandleError::NotSupported)
    }
}

impl HasDisplayHandle for WasdkWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, HandleError> {
        Err(HandleError::NotSupported)
    }
}

impl PlatformWindow for WasdkWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.shared.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        self.shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
            .is_some_and(|window| window.is_maximized())
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.shared.state.borrow().bounds.size
    }

    fn resize(&mut self, new_size: Size<Pixels>) {
        let view = self.shared.view.borrow();
        let Some(window) = view
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        else {
            self.shared.state.borrow_mut().bounds.size = new_size;
            return;
        };
        let mut frame = window.frame();
        frame.size.width = f64::from(f32::from(new_size.width));
        frame.size.height = f64::from(f32::from(new_size.height));
        window.set_frame(frame, None);
    }

    fn scale_factor(&self) -> f32 {
        self.shared.state.borrow().scale
    }

    fn appearance(&self) -> WindowAppearance {
        self.shared.appearance.get()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::clone(&self.shared.display))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.shared.state.borrow().pointer.position()
    }

    fn modifiers(&self) -> Modifiers {
        self.shared.state.borrow().keys.modifiers()
    }

    fn capslock(&self) -> Capslock {
        self.shared.state.borrow().keys.capslock()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.shared.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.shared.state.borrow_mut().input_handler.take()
    }

    /// Kept for the input method, which hears it after the frame GPUI says it in.
    fn set_text_input_configuration(&mut self, configuration: TextInputConfiguration) {
        self.shared.state.borrow_mut().text_input_configuration = configuration;
    }

    /// None: GPUI draws its own prompt in the window, as on the web.
    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self) {
        if let Some(window) = self
            .shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.show();
        }
    }

    fn is_active(&self) -> bool {
        self.shared.state.borrow().active
    }

    /// Visible: WaOS sends a view off screen no frames, which is what GPUI wants of a hidden one.
    fn visibility(&self) -> WindowVisibility {
        WindowVisibility::Visible
    }

    fn is_hovered(&self) -> bool {
        self.shared.state.borrow().hovered
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_title(&mut self, title: &str) {
        self.shared.state.borrow_mut().title = title.to_owned();
        if let Some(window) = self
            .shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.set_title(title);
        }
    }

    fn get_title(&self) -> String {
        self.shared.state.borrow().title.clone()
    }

    fn set_background_appearance(&self, _background_appearance: WindowBackgroundAppearance) {}

    fn minimize(&self) {
        if let Some(window) = self
            .shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.set_minimized(true);
        }
    }

    fn zoom(&self) {
        if let Some(window) = self
            .shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.set_maximized(!window.is_maximized());
        }
    }

    fn toggle_fullscreen(&self) {
        if let Some(window) = self
            .shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.set_fullscreen(!window.is_fullscreen(), None);
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
            .is_some_and(|window| window.is_fullscreen())
    }

    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        let shared: Weak<WindowShared> = Rc::downgrade(&self.shared);
        Some(Rc::new(move || {
            if let Some(shared) = shared.upgrade() {
                shared.request_frame();
            }
        }))
    }

    fn schedule_frame(&self) {
        self.shared.request_frame();
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.shared.callbacks.request_frame.set(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.shared.callbacks.input.set(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.shared.callbacks.active_status_change.set(callback);
    }

    fn on_visibility_change(&self, _callback: Box<dyn FnMut(WindowVisibility)>) {}

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.shared.callbacks.hover_status_change.set(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.shared.callbacks.resize.set(callback);
    }

    fn insets(&self) -> WindowInsets {
        self.shared.state.borrow().insets.clone()
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(WindowInsets)>) {
        self.shared.callbacks.insets_changed.set(callback);
    }

    /// WaOS reports no move, so GPUI hears of none.
    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_display_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.shared.callbacks.should_close.set(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.shared.callbacks.close.set(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.shared.callbacks.appearance_changed.set(callback);
    }

    fn draw(&self, scene: &Scene) {
        if let Some(attached) = self.shared.view.borrow().as_ref() {
            attached.renderer.borrow_mut().draw(scene);
        }
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        Arc::clone(&self.shared.atlas) as Arc<dyn PlatformAtlas>
    }

    /// The host draws glyphs as coverage, so no LCD text.
    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.renderer.borrow().gpu_specs())
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn start_window_move(&self) {
        if let Some(window) = self
            .shared
            .view
            .borrow()
            .as_ref()
            .and_then(|attached| attached.system_window.as_ref())
        {
            window.drag();
        }
    }
}

/// Where the platform's UI covers a view of `metrics`, at `scale` physical pixels to a logical one:
/// its safe area what covers it now, the status bar, a notch, the navigation bar (Flutter's
/// `padding`), and its keyboard what covers it completely (`viewInsets`).
fn insets_of(metrics: &Metrics, scale: f32) -> WindowInsets {
    WindowInsets {
        safe_area: edges_of(&metrics.padding, scale),
        ime: edges_of(&metrics.view_insets, scale),
        ..WindowInsets::default()
    }
}

fn edges_of(insets: &Insets, scale: f32) -> Edges<Pixels> {
    let logical = |physical: f64| px(physical as f32 / scale);
    Edges {
        top: logical(insets.top),
        right: logical(insets.right),
        bottom: logical(insets.bottom),
        left: logical(insets.left),
    }
}
