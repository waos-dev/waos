//! egui on a view of WaOS: what `egui-winit` is to a winit window.
//!
//! A view's events are translated as they arrive and kept until the next pass; each asks the view
//! for a frame, and the pass runs when that frame comes, so nothing is laid out or drawn while
//! nothing happens. After a pass, what egui asked of the platform is carried out: the cursor's
//! shape, a copy, the input method's start and stop, and the next frame, at once or after a wait.
//!
//! The invariant: positions and sizes cross here, a view's in physical pixels and egui's in
//! points, `device-pixel-ratio` of the first to one of the second.

mod clipboard;
mod cursor;
mod keys;
mod pointer;
mod repaint;
mod text;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use egui::{Event, Key, KeyboardShortcut, Modifiers, ViewportId};
use wasdk::views::{EditCommand, KeyEvent, KeyInput, KeyKind, ViewEvent, ViewFocusState};
use wasdk_wgpu::SharedView;

use self::clipboard::PendingCopy;
use self::keys::Keyboard;
use self::pointer::Pointers;
use self::repaint::RepaintTimer;
use self::text::TextSession;

/// A view's side of an egui context.
pub struct EguiView {
    context: egui::Context,
    view: SharedView,
    keyboard: Keyboard,
    pointers: Pointers,
    text: TextSession,
    copy: PendingCopy,
    /// What egui has not heard yet. Shared with the tasks that read a paste.
    events: Rc<RefCell<Vec<Event>>>,
    focused: bool,
    theme: egui::Theme,
    cursor: egui::CursorIcon,
    shortcuts: Vec<KeyboardShortcut>,
    timer: RepaintTimer,
}

/// What a pass left to draw.
pub struct Painting {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

impl EguiView {
    /// An egui context fed from `view`, on macOS or not: where ⌘ is the command key.
    pub fn new(view: SharedView, macos: bool) -> EguiView {
        EguiView {
            context: egui::Context::default(),
            text: TextSession::new(&view),
            view,
            keyboard: Keyboard::new(macos),
            pointers: Pointers::new(),
            copy: PendingCopy::default(),
            events: Rc::default(),
            focused: true,
            theme: egui::Theme::Light,
            cursor: egui::CursorIcon::Default,
            shortcuts: Vec::new(),
            timer: RepaintTimer::default(),
        }
    }

    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// The chords the app answers itself, which the platform is told were handled.
    pub fn claim_shortcuts(&mut self, shortcuts: Vec<KeyboardShortcut>) {
        self.shortcuts = shortcuts;
    }

    /// The system's light or dark, which egui's own follows.
    pub fn set_theme(&mut self, theme: egui::Theme) {
        self.theme = theme;
        self.view.request_frame();
    }

    /// Takes in an event of the view's input, and asks the view for the frame egui hears it in.
    /// Frames, metrics, Back and requests to close are the app's own, and are heard as nothing
    /// here.
    pub fn hear(&mut self, event: ViewEvent) {
        match event {
            ViewEvent::Pointer(pointer) => {
                let modifiers = self.keyboard.modifiers();
                let events = self
                    .pointers
                    .translate(&pointer, self.pixels_per_point(), modifiers);
                self.events.borrow_mut().extend(events);
            }
            ViewEvent::Key(key) => self.hear_key(key),
            ViewEvent::Focus(focus) => {
                self.focused = focus.state == ViewFocusState::Focused;
                self.push(Event::WindowFocused(self.focused));
            }
            ViewEvent::TextInputEditingValue(value) => {
                if let Some(event) = self.text.value_changed(value) {
                    self.push(event);
                }
            }
            ViewEvent::TextInputAction(_) => self.press(Key::Enter, Modifiers::NONE),
            ViewEvent::TextInputClosed => self.text.closed(),
            ViewEvent::Copy(request) => {
                self.copy.wait_with(request);
                self.push(Event::Copy);
            }
            ViewEvent::Cut(request) => {
                self.copy.wait_with(request);
                self.push(Event::Cut);
            }
            ViewEvent::Paste(paste) => self.hear_paste(paste),
            ViewEvent::Edit(command) => self.hear_edit(command),
            ViewEvent::Frame(_)
            | ViewEvent::Metrics(_)
            | ViewEvent::CloseRequested
            | ViewEvent::PopRoute => return,
        }
        self.view.request_frame();
    }

    /// Runs one pass of `ui` over what was heard since the last, at the frame's `time` in
    /// nanoseconds, and carries out what egui asked of the platform in it.
    pub fn run(
        &mut self,
        time: u64,
        max_texture_side: u32,
        ui: impl FnMut(&mut egui::Ui),
    ) -> Painting {
        let input = self.input(time, max_texture_side);
        let output = self.context.run_ui(input, ui);
        let pixels_per_point = output.pixels_per_point;
        self.carry_out(output.platform_output, pixels_per_point);
        self.request_next_frame(&output.viewport_output);
        Painting {
            primitives: self.context.tessellate(output.shapes, pixels_per_point),
            textures: output.textures_delta,
            pixels_per_point,
        }
    }

    fn pixels_per_point(&self) -> f32 {
        self.view.metrics().device_pixel_ratio as f32
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

    /// A key goes to egui, with the text it types; the platform is told at once whether egui, or
    /// a shortcut of the app's, takes it, from what egui wanted at its last pass, since the pass
    /// that hears this key has not run yet.
    fn hear_key(&mut self, input: KeyInput) {
        let KeyInput { event, reply } = input;
        if self.keyboard.track(&event) {
            self.push(Event::ModifiersChanged(self.keyboard.modifiers()));
            return;
        }
        let modifiers = self.keyboard.modifiers();
        let key = keys::key_of(event.logical);
        let pressed = event.kind != KeyKind::Up;
        if let Some(key) = key {
            self.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: event.kind == KeyKind::Repeat,
                modifiers,
            });
        }
        if let Some(text) = typed_text(&event, modifiers) {
            self.push(Event::Text(text));
        }
        reply.handled(key.is_some_and(|key| self.takes(key, modifiers)));
    }

    /// Whether `key` is the app's or egui's to answer. Copy, cut and paste are left to the
    /// platform, which sends them back as the view's `copy`, `cut` and `paste`, each with what
    /// carries it out.
    fn takes(&self, key: Key, modifiers: Modifiers) -> bool {
        let claimed = |shortcut: &KeyboardShortcut| {
            shortcut.logical_key == key && modifiers.matches_exact(shortcut.modifiers)
        };
        if self.shortcuts.iter().any(claimed) {
            return true;
        }
        let clipboard_chord = modifiers.command && matches!(key, Key::C | Key::X | Key::V);
        let listening = self.context.egui_wants_keyboard_input()
            || self.context.memory(|memory| memory.focused().is_some());
        listening && !clipboard_chord
    }

    fn hear_paste(&mut self, paste: wasdk::clipboard::Paste) {
        let (events, view) = (self.events.clone(), self.view.clone());
        wasdk::spawn(async move {
            if let Some(text) = clipboard::pasted_text(paste).await {
                events.borrow_mut().push(Event::Paste(text));
                view.request_frame();
            }
        });
    }

    /// One of Edit's commands, chosen in the platform's menu, as the chord egui knows it by.
    fn hear_edit(&mut self, command: EditCommand) {
        let (key, modifiers) = match command {
            EditCommand::Undo => (Key::Z, Modifiers::COMMAND),
            EditCommand::Redo => (Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
            EditCommand::SelectAll => (Key::A, Modifiers::COMMAND),
            EditCommand::Delete => (Key::Delete, Modifiers::NONE),
        };
        self.press(key, modifiers);
    }

    fn input(&mut self, time: u64, max_texture_side: u32) -> egui::RawInput {
        let metrics = self.view.metrics();
        let pixels_per_point = metrics.device_pixel_ratio as f32;
        let size = egui::vec2(metrics.width as f32, metrics.height as f32) / pixels_per_point;
        let viewport = egui::ViewportInfo {
            native_pixels_per_point: Some(pixels_per_point),
            focused: Some(self.focused),
            ..Default::default()
        };
        let covered = metrics.padding;
        let safe_area = egui::epaint::MarginF32 {
            left: covered.left as f32,
            right: covered.right as f32,
            top: covered.top as f32,
            bottom: covered.bottom as f32,
        } / pixels_per_point;
        egui::RawInput {
            viewport_id: ViewportId::ROOT,
            viewports: std::iter::once((ViewportId::ROOT, viewport)).collect(),
            safe_area_insets: Some(egui::SafeAreaInsets(safe_area)),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            max_texture_side: Some(max_texture_side as usize),
            time: Some(Duration::from_nanos(time).as_secs_f64()),
            events: std::mem::take(&mut *self.events.borrow_mut()),
            focused: self.focused,
            system_theme: Some(self.theme),
            ..Default::default()
        }
    }

    /// What egui asked of the platform in a pass: the cursor, copies, links and text input.
    fn carry_out(&mut self, output: egui::PlatformOutput, pixels_per_point: f32) {
        if output.cursor_icon != self.cursor {
            self.cursor = output.cursor_icon;
            self.view.set_cursor(cursor::cursor_of(self.cursor));
        }
        for command in output.commands {
            match command {
                egui::OutputCommand::CopyText(text) => self.copy.write(text),
                egui::OutputCommand::OpenUrl(open) => open_link(open.url),
                egui::OutputCommand::CopyImage(_)
                | egui::OutputCommand::TextSelectionSettled(_) => {}
            }
        }
        self.copy.forget();
        self.text.follow(output.ime, pixels_per_point);
    }

    fn request_next_frame(&self, viewports: &egui::OrderedViewportIdMap<egui::ViewportOutput>) {
        let Some(delay) = viewports
            .get(&ViewportId::ROOT)
            .map(|root| root.repaint_delay)
        else {
            return;
        };
        if delay.is_zero() {
            self.view.request_frame();
        } else if delay < Duration::MAX {
            self.timer.request_frame_after(&self.view, delay);
        }
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
            eprintln!("model-viewer: {link} was not opened: {error}");
        }
    });
}
