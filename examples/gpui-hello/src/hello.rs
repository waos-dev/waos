//! The app: text, a button that counts its clicks, a text field, and the keys pressed last.

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, Keystroke, MouseButton, MouseUpEvent,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

use crate::text_field::{self, TextInput};

struct Hello {
    field: Entity<TextInput>,
    clicks: usize,
    keystrokes: Vec<Keystroke>,
    focus_handle: FocusHandle,
}

impl Hello {
    fn clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.clicks += 1;
        cx.notify();
    }
}

impl Focusable for Hello {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Hello {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.field.read(cx).text();
        div()
            .track_focus(&self.focus_handle(cx))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .size_full()
            .bg(rgb(0x1e1f22))
            .text_color(rgb(0xdfe1e5))
            .child(div().text_2xl().child("Hello from GPUI in WaOS"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .id("button")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x3574f0))
                            .text_color(rgb(0xffffff))
                            .hover(|style| style.bg(rgb(0x4682fa)).cursor_pointer())
                            .child("Click me")
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::clicked)),
                    )
                    .child(format!("clicked {} times", self.clicks)),
            )
            .child(div().text_color(rgb(0x1e1f22)).child(self.field.clone()))
            .child(format!("typed: {typed}"))
            .child(
                div()
                    .font_family("Menlo")
                    .text_sm()
                    .text_color(rgb(0x9da0a8))
                    .children(self.keystrokes.iter().rev().take(8).map(|keystroke| {
                        format!(
                            "{} {}",
                            keystroke.unparse(cx.platform_info().os),
                            keystroke
                                .key_char
                                .as_ref()
                                .map(|text| format!("-> {text:?}"))
                                .unwrap_or_default()
                        )
                    })),
            )
    }
}

pub(crate) fn launch(cx: &mut App) {
    cx.bind_keys(text_field::key_bindings());
    text_field::on_quit(cx);
    let bounds = Bounds::centered(None, size(px(520.), px(420.)), cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("GPUI hello".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                let field = cx.new(|cx| TextInput::new("Type here...", cx));
                cx.new(|cx| Hello {
                    field,
                    clicks: 0,
                    keystrokes: Vec::new(),
                    focus_handle: cx.focus_handle(),
                })
            },
        )
        .expect("the window opens");
    let view = window
        .update(cx, |_, _, cx| cx.entity())
        .expect("the window is there");
    cx.observe_keystrokes(move |event, _, cx| {
        view.update(cx, |view, cx| {
            view.keystrokes.push(event.keystroke.clone());
            cx.notify();
        })
    })
    .detach();
    window
        .update(cx, |hello, window, cx| {
            window.focus(&hello.field.focus_handle(cx), cx);
        })
        .expect("the window is there");
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}
