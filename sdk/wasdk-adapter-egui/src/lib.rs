//! egui on a view of WaOS: what `egui-winit` is to a winit window, with its names.
//!
//! A view's events are translated as they arrive and kept until the next pass
//! ([`State::on_view_event`], [`State::take_egui_input`]). After a pass, what egui asked of the
//! system is carried out on the view ([`State::handle_platform_output`]): the cursor's shape, a
//! copy, the input method's start and stop, a link to open. Running the pass, drawing it and
//! asking for the next frame are the app's, as they are an integration's over `egui-winit`.
//!
//! An event is handed in and handed back, where `egui-winit` borrows one: a view's key carries the
//! reply the system waits on, and its copy and paste what carries them out, so egui can only use
//! an event up by owning it.
//!
//! The invariant: positions and sizes cross here, a view's in physical pixels and egui's in
//! points, `device-pixel-ratio` of the first to one of the second.

mod clipboard;
mod cursor;
mod keys;
mod pointer;
mod text;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use egui::{Event, Key, Modifiers, ViewportId};
use wasdk::system::{self, OperatingSystem};
use wasdk::views::{EditCommand, KeyEvent, KeyInput, KeyKind, View, ViewEvent, ViewFocusState};

use self::clipboard::PendingCopy;
use self::keys::Keyboard;
use self::pointer::Pointers;
use self::text::TextSession;

/// egui's side of one view: what `egui_winit::State` is for a winit window.
pub struct State {
    egui_ctx: egui::Context,
    view: Arc<View>,
    start_time: Instant,
    keyboard: Keyboard,
    pointers: Pointers,
    text: TextSession,
    copy: PendingCopy,
    /// What egui has not been handed yet. Shared with the tasks that read a paste.
    events: Rc<RefCell<Vec<Event>>>,
    focused: bool,
    theme: Option<egui::Theme>,
    cursor: egui::CursorIcon,
    max_texture_side: Option<usize>,
}

/// What became of an event handed to [`State::on_view_event`].
pub struct EventResponse {
    /// The event, unless egui used it up: the app's to act on. A key comes back with its reply
    /// still to give.
    pub event: Option<ViewEvent>,
    /// Whether egui wants a new frame for it.
    pub repaint: bool,
}

impl EventResponse {
    /// Whether egui used the event up, as `egui_winit`'s `consumed`: the app does not act on it.
    pub fn consumed(&self) -> bool {
        self.event.is_none()
    }
}

impl State {
    /// egui's side of `view`, feeding `egui_ctx`.
    pub fn new(egui_ctx: egui::Context, view: Arc<View>) -> State {
        State {
            egui_ctx,
            text: TextSession::new(&view),
            view,
            start_time: Instant::now(),
            keyboard: Keyboard::new(system::os() == OperatingSystem::Macos),
            pointers: Pointers::new(),
            copy: PendingCopy::default(),
            events: Rc::default(),
            focused: true,
            theme: None,
            cursor: egui::CursorIcon::Default,
            max_texture_side: None,
        }
    }

    pub fn egui_ctx(&self) -> &egui::Context {
        &self.egui_ctx
    }

    /// The largest texture the app's GPU device takes on a side, which egui keeps its atlas
    /// under.
    pub fn set_max_texture_side(&mut self, max_texture_side: usize) {
        self.max_texture_side = Some(max_texture_side);
    }

    /// The system's light or dark, which egui's own follows. A view's events do not carry it:
    /// the app reads it from `wasdk::system`.
    pub fn set_theme(&mut self, theme: egui::Theme) {
        self.theme = Some(theme);
    }

    /// Takes one of the view's events in, as `egui_winit`'s `on_window_event`, and answers it
    /// back unless egui used it up. Frames, metrics, Back and requests to close are the app's
    /// own, and come back untouched.
    pub fn on_view_event(&mut self, event: ViewEvent) -> EventResponse {
        let event = match event {
            ViewEvent::Pointer(pointer) => {
                let modifiers = self.keyboard.modifiers();
                let pixels_per_point = self.pixels_per_point();
                let events = self
                    .pointers
                    .translate(&pointer, pixels_per_point, modifiers);
                self.events.borrow_mut().extend(events);
                let over_egui = self.egui_ctx.egui_wants_pointer_input();
                (!over_egui).then_some(ViewEvent::Pointer(pointer))
            }
            ViewEvent::Key(key) => self.on_key(key).map(ViewEvent::Key),
            ViewEvent::Focus(focus) => {
                self.focused = focus.state == ViewFocusState::Focused;
                self.push(Event::WindowFocused(self.focused));
                Some(ViewEvent::Focus(focus))
            }
            ViewEvent::TextInputEditingValue(value) => {
                if let Some(event) = self.text.value_changed(value) {
                    self.push(event);
                }
                None
            }
            ViewEvent::TextInputAction(_) => {
                self.press(Key::Enter, Modifiers::NONE);
                None
            }
            ViewEvent::TextInputClosed => {
                self.text.closed();
                None
            }
            ViewEvent::Copy(request) if self.wants_keyboard() => {
                self.copy.wait_with(request);
                self.push(Event::Copy);
                None
            }
            ViewEvent::Cut(request) if self.wants_keyboard() => {
                self.copy.wait_with(request);
                self.push(Event::Cut);
                None
            }
            ViewEvent::Paste(paste) if self.wants_keyboard() => {
                self.read_paste(paste);
                None
            }
            ViewEvent::Edit(command) if self.wants_keyboard() => {
                self.on_edit(command);
                None
            }
            // The app's own: a frame, the view's geometry, Back and a request to close, and the
            // clipboard's and Edit's commands while no field of egui's has the keyboard.
            untouched => {
                return EventResponse {
                    event: Some(untouched),
                    repaint: false,
                };
            }
        };
        EventResponse {
            event,
            repaint: true,
        }
    }

    /// The input gathered since the last pass, as `egui_winit`'s `take_egui_input`.
    pub fn take_egui_input(&mut self) -> egui::RawInput {
        let metrics = self.view.metrics();
        let pixels_per_point = metrics.device_pixel_ratio as f32;
        let size = egui::vec2(metrics.width as f32, metrics.height as f32) / pixels_per_point;
        let viewport = egui::ViewportInfo {
            native_pixels_per_point: Some(pixels_per_point),
            focused: Some(self.focused),
            ..Default::default()
        };
        egui::RawInput {
            viewport_id: ViewportId::ROOT,
            viewports: std::iter::once((ViewportId::ROOT, viewport)).collect(),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            max_texture_side: self.max_texture_side,
            time: Some(self.start_time.elapsed().as_secs_f64()),
            events: std::mem::take(&mut *self.events.borrow_mut()),
            focused: self.focused,
            system_theme: self.theme,
            ..Default::default()
        }
    }

    /// Carries out what egui asked of the system in a pass, as `egui_winit`'s
    /// `handle_platform_output`: the cursor's shape, a copy, a link to open, and the input
    /// method's start and stop with where the caret is.
    pub fn handle_platform_output(&mut self, platform_output: egui::PlatformOutput) {
        if platform_output.cursor_icon != self.cursor {
            self.cursor = platform_output.cursor_icon;
            self.view.set_cursor(cursor::cursor_of(self.cursor));
        }
        for command in platform_output.commands {
            match command {
                egui::OutputCommand::CopyText(text) => self.copy.write(text),
                egui::OutputCommand::OpenUrl(open) => open_link(open.url),
                egui::OutputCommand::CopyImage(_) => {}
            }
        }
        self.copy.forget();
        let pixels_per_point = self.pixels_per_point();
        self.text.follow(platform_output.ime, pixels_per_point);
    }

    fn pixels_per_point(&self) -> f32 {
        self.view.metrics().device_pixel_ratio as f32
    }

    /// Whether a field of egui's has the keyboard: what makes a key, the clipboard's commands
    /// and Edit's egui's and not the app's.
    fn wants_keyboard(&self) -> bool {
        self.egui_ctx.egui_wants_keyboard_input()
    }

    fn push(&self, event: Event) {
        self.events.borrow_mut().push(event);
    }

    /// A key pressed and released at once: how a command with no key behind it reaches egui.
    fn press(&self, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
    }

    /// A key goes to egui, with the text it types. Where a field of egui's has the keyboard the
    /// key is used up and the system told so at once, from what egui wanted at its last pass,
    /// since the pass that takes this key has not run yet; otherwise it comes back for the app,
    /// its reply still to give. Copy, cut and paste are left to the system either way, which
    /// sends them back as the view's `copy`, `cut` and `paste`, each with what carries it out.
    fn on_key(&mut self, input: KeyInput) -> Option<KeyInput> {
        if self.keyboard.track(&input.event) {
            self.push(Event::ModifiersChanged(self.keyboard.modifiers()));
            return Some(input);
        }
        let modifiers = self.keyboard.modifiers();
        let key = keys::key_of(input.event.logical);
        if let Some(key) = key {
            self.push(Event::Key {
                key,
                physical_key: None,
                pressed: input.event.kind != KeyKind::Up,
                repeat: input.event.kind == KeyKind::Repeat,
                modifiers,
            });
        }
        if let Some(text) = typed_text(&input.event, modifiers) {
            self.push(Event::Text(text));
        }
        if !self.wants_keyboard() {
            return Some(input);
        }
        let clipboard_chord = modifiers.command && matches!(key, Some(Key::C | Key::X | Key::V));
        input.reply.handled(!clipboard_chord);
        None
    }

    /// Reads what the user pasted, which reaches egui as a paste once it is read.
    fn read_paste(&mut self, paste: wasdk::clipboard::Paste) {
        let (events, view) = (Rc::clone(&self.events), self.view.clone());
        wasdk::spawn(async move {
            if let Some(text) = clipboard::pasted_text(paste).await {
                events.borrow_mut().push(Event::Paste(text));
                view.request_frame();
            }
        });
    }

    /// One of Edit's commands, chosen in the system's menu, as the chord egui knows it by.
    fn on_edit(&mut self, command: EditCommand) {
        let (key, modifiers) = match command {
            EditCommand::Undo => (Key::Z, Modifiers::COMMAND),
            EditCommand::Redo => (Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
            EditCommand::SelectAll => (Key::A, Modifiers::COMMAND),
            EditCommand::Delete => (Key::Delete, Modifiers::NONE),
        };
        self.press(key, modifiers);
    }
}

/// The text a key press types: its character, unless it is a control character or a chord's.
fn typed_text(event: &KeyEvent, modifiers: Modifiers) -> Option<String> {
    if event.kind == KeyKind::Up || modifiers.command || modifiers.ctrl {
        return None;
    }
    let text = event.character.as_ref()?;
    let printable = text.chars().all(|character| !character.is_control());
    (printable && !text.is_empty()).then(|| text.clone())
}

fn open_link(link: String) {
    let Some(opener) = wasdk::opener::get_opener() else {
        return;
    };
    wasdk::spawn(async move {
        if let Err(error) = opener.open_link(link.clone()).await {
            tracing::warn!("{link} was not opened: {error}");
        }
    });
}
