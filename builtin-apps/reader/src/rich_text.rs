//! A run of an item's spans as one piece of text: plain, strong, emphasised and code spans in
//! their styles, and a link's words in the accent colour, which a click opens in the system's
//! browser. Each kind of block draws its words through this, in its own base style.
//!
//! The invariant a later editor will break: the widget owns one tap recognizer to each link span,
//! made when its spans arrive and disposed of when they change or the widget goes, as a
//! recognizer on a text span is its owner's to dispose of.

use waft::ui::{FontStyle, FontWeight, TextAlign};
use waft::{
    App, BuildContext, Handle, IntoWidget, Listener, State, StateData, StatefulWidget, Text,
    TextSpan, TextStyle, WidgetRef,
};
use waft_gestures::{GestureRecognizerLeaf, TapGestureRecognizer};
use waft_services::SystemMouseCursors;
use waft_winui::{ThemeResources, control_text_style};

use crate::article::Span;
use crate::opening_links::open_link;
use crate::reader_app::ReaderApp;

/// The families code is set in, the first the system has.
const CODE_FAMILIES: [&str; 5] = ["SF Mono", "Menlo", "Cascadia Mono", "Consolas", "monospace"];

/// The size the item's text is set at, in logical points.
const BODY_SIZE: f64 = 16.0;

/// How large code within a line is set beside the words around it: a monospace face reads larger
/// than the text face at one size.
const CODE_SCALE: f64 = 0.9;

/// The base style a run of spans is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TextKind {
    /// The item's text: a paragraph or a list item.
    Body,
    /// A heading, from 1, the largest, to 6.
    Heading(u8),
    Quote,
    Caption,
}

impl TextKind {
    pub(crate) fn style(self, resources: &ThemeResources) -> TextStyle {
        let primary = resources.text_fill_color_primary;
        let secondary = resources.text_fill_color_secondary;
        let style = |size, weight, colour, height| {
            control_text_style(resources, size, weight, colour).height(height)
        };
        match self {
            TextKind::Body => style(BODY_SIZE, FontWeight::NORMAL, primary, 1.5),
            TextKind::Heading(1 | 2) => style(22.0, FontWeight::W600, primary, 1.3),
            TextKind::Heading(3) => style(18.0, FontWeight::W600, primary, 1.35),
            TextKind::Heading(_) => style(16.0, FontWeight::W600, primary, 1.4),
            TextKind::Quote => style(BODY_SIZE, FontWeight::NORMAL, secondary, 1.5),
            TextKind::Caption => style(13.0, FontWeight::NORMAL, secondary, 1.4),
        }
    }
}

/// The style code is set in, on `base`.
pub(crate) fn code_style(base: TextStyle) -> TextStyle {
    let fallbacks = CODE_FAMILIES[1..].iter().map(|family| (*family).to_owned());
    base.font_family(CODE_FAMILIES[0])
        .font_family_fallback(fallbacks.collect())
}

#[derive(Debug)]
pub(crate) struct RichText {
    pub(crate) reader: ReaderApp,
    pub(crate) spans: Vec<Span>,
    pub(crate) kind: TextKind,
    pub(crate) align: TextAlign,
}

impl StatefulWidget for RichText {
    type State = RichTextState;

    fn create_state(&self) -> RichTextState {
        RichTextState {
            state: StateData::new(),
            link_taps: Vec::new(),
        }
    }
}

pub(crate) struct RichTextState {
    state: StateData<RichText>,
    /// The tap recognizer of each span that is a link, in the spans' order.
    link_taps: Vec<Handle<TapGestureRecognizer>>,
}

impl RichTextState {
    /// Makes a tap recognizer for each link among the widget's spans, which opens it.
    fn hear_link_taps(self: Handle<Self>, app: &mut App) {
        let widget = self.widget(app);
        let reader = widget.reader.clone();
        let links: Vec<String> = widget
            .spans
            .iter()
            .filter_map(|span| span.style.link.clone())
            .collect();
        let taps = links.into_iter().map(|link| {
            let reader = reader.clone();
            let tap = TapGestureRecognizer::new(app);
            let open = Listener::new(move |app: &mut App| open_link(app, &reader, &link));
            tap.set_on_tap(app, Some(open));
            tap
        });
        let taps = taps.collect();
        app.get_mut(self).link_taps = taps;
    }

    fn dispose_link_taps(self: Handle<Self>, app: &mut App) {
        for tap in std::mem::take(&mut app.get_mut(self).link_taps) {
            tap.dispose(app);
        }
    }
}

impl State for RichTextState {
    type Widget = RichText;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        self.hear_link_taps(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, old_widget: &RichText) {
        if old_widget.spans != self.widget(app).spans {
            self.dispose_link_taps(app);
            self.hear_link_taps(app);
        }
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        self.dispose_link_taps(app);
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let widget = self.widget(app);
        let base = widget.kind.style(&resources);
        let mut link_taps = app.get(self).link_taps.iter();
        let children = widget.spans.iter().map(|span| {
            let styled = TextSpan::new()
                .text(span.text.clone())
                .style(span_style(&resources, &base, span));
            let tap = span.style.link.as_ref().and_then(|_| link_taps.next());
            match tap {
                Some(tap) => styled
                    .recognizer(tap.as_recognizer())
                    .mouse_cursor(SystemMouseCursors::CLICK),
                None => styled,
            }
            .into_span()
        });
        let text = TextSpan::new()
            .style(base.clone())
            .children(children.collect());
        Text::rich(text.into_span())
            .text_align(widget.align)
            .into_widget()
    }
}

/// `base`, with what `span`'s own style adds to it.
fn span_style(resources: &ThemeResources, base: &TextStyle, span: &Span) -> TextStyle {
    let mut style = base.clone();
    if span.style.strong {
        style = style.font_weight(FontWeight::W600);
    }
    if span.style.emphasised {
        style = style.font_style(FontStyle::Italic);
    }
    if span.style.code {
        let size = base.font_size.unwrap_or(BODY_SIZE) * CODE_SCALE;
        style = code_style(style)
            .font_size(size)
            .background_color(resources.subtle_fill_color_secondary);
    }
    if span.style.link.is_some() {
        style = style.color(resources.accent_text_fill_color_primary_brush);
    }
    style
}
