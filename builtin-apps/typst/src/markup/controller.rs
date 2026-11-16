//! The editor's text controller: Waft's, with the text drawn as marked-up source, each run in its
//! colour, the stretches where Typst found a problem underlined, and the text an input method is
//! composing underlined as any text field underlines it.
//!
//! The runs are parsed from the text when it is drawn after a change, and kept until the next
//! change, so moving the caret parses nothing. The problems' places are those of the last run of
//! the compiler, which follows the text by a moment: a place past the text's end is left out.

use std::cell::RefCell;
use std::ops::Range;

use waft::{
    App, BuildContext, ChangeNotifier, ChangeNotifierData, Handle, InlineSpanRef, Ref,
    TextDecoration, TextEditingControllerData, TextEditingControllerLeaf, TextSpan, TextStyle,
};

use super::marks::{Run, runs_of};
use super::palette::Palette;
use super::utf16::byte_of;

pub(crate) struct MarkupController {
    change_notifier: ChangeNotifierData,
    text_editing_controller: TextEditingControllerData,
    palette: Palette,
    /// Where the compiler found problems, in bytes of the text it was given.
    problems: Vec<Range<usize>>,
    parsed: RefCell<Parsed>,
}

/// The runs of the text they were parsed from.
#[derive(Default)]
struct Parsed {
    text: String,
    runs: Vec<Run>,
}

impl MarkupController {
    pub(crate) fn new(app: &mut App) -> Ref<MarkupController> {
        app.create(MarkupController {
            change_notifier: ChangeNotifierData::new(),
            text_editing_controller: TextEditingControllerData::new(Default::default()),
            palette: Palette::LIGHT,
            problems: Vec::new(),
            parsed: RefCell::default(),
        })
    }

    /// Draws the markup in `palette` from the next time it is drawn.
    pub(crate) fn set_palette(self: Handle<Self>, app: &mut App, palette: Palette) {
        app.get_mut(self).palette = palette;
    }

    /// Underlines `problems`, places in UTF-16 code units of the text as it is now, and draws
    /// the text again.
    pub(crate) fn set_problems(self: Handle<Self>, app: &mut App, problems: &[Range<usize>]) {
        let text = self.text_value(app);
        let problems: Vec<Range<usize>> = problems
            .iter()
            .map(|units| byte_of(text, units.start)..byte_of(text, units.end))
            .collect();
        if app.get(self).problems == problems {
            return;
        }
        app.get_mut(self).problems = problems;
        self.notify_listeners(app);
    }

    /// The runs of `text`, parsed where it changed since they were last asked for.
    fn runs(&self, text: &str) -> Vec<Run> {
        let mut parsed = self.parsed.borrow_mut();
        if parsed.text != text {
            *parsed = Parsed {
                text: text.to_owned(),
                runs: runs_of(text),
            };
        }
        parsed.runs.clone()
    }
}

impl ChangeNotifier for MarkupController {
    fn change_notifier_data(&self) -> &ChangeNotifierData {
        &self.change_notifier
    }

    fn change_notifier_data_mut(&mut self) -> &mut ChangeNotifierData {
        &mut self.change_notifier
    }
}

impl TextEditingControllerLeaf for MarkupController {
    fn text_editing_controller_data(self: Handle<Self>, app: &App) -> &TextEditingControllerData {
        &app.get(self).text_editing_controller
    }

    fn text_editing_controller_data_mut(
        self: Handle<Self>,
        app: &mut App,
    ) -> &mut TextEditingControllerData {
        &mut app.get_mut(self).text_editing_controller
    }

    fn build_text_span(
        self: Handle<Self>,
        app: &App,
        _context: BuildContext,
        style: Option<TextStyle>,
        with_composing: bool,
    ) -> TextSpan {
        let controller = app.get(self);
        let value = self.value(app);
        let text = value.text.as_str();
        let composing = (with_composing && value.is_composing_range_valid()).then(|| {
            let units = value.composing;
            byte_of(text, units.start as usize)..byte_of(text, units.end as usize)
        });
        let underlined = Underlined {
            problems: &controller.problems,
            composing,
        };
        let spans = controller
            .runs(text)
            .into_iter()
            .flat_map(|run| underlined.pieces_of(run))
            .map(|piece| piece.span(text, &controller.palette))
            .collect();
        let span = TextSpan::new().children(spans);
        match style {
            Some(style) => span.style(style),
            None => span,
        }
    }
}

/// The stretches of the text that are underlined, whatever they are.
struct Underlined<'a> {
    problems: &'a [Range<usize>],
    composing: Option<Range<usize>>,
}

/// A run, or the part of one, that is drawn in one style.
struct Piece {
    run: Run,
    problem: bool,
    composing: bool,
}

impl Underlined<'_> {
    /// `run` cut where an underline starts or ends inside it.
    fn pieces_of(&self, run: Run) -> Vec<Piece> {
        let mut cuts = vec![run.bytes.start, run.bytes.end];
        let ranges = self.problems.iter().chain(&self.composing);
        for edge in ranges.flat_map(|range| [range.start, range.end]) {
            if run.bytes.start < edge && edge < run.bytes.end {
                cuts.push(edge);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        cuts.windows(2)
            .map(|cut| self.piece(cut[0]..cut[1], &run))
            .collect()
    }

    fn piece(&self, bytes: Range<usize>, run: &Run) -> Piece {
        let within = |range: &Range<usize>| range.start <= bytes.start && bytes.end <= range.end;
        Piece {
            problem: self.problems.iter().any(within),
            composing: self.composing.as_ref().is_some_and(within),
            run: Run {
                bytes,
                tag: run.tag,
            },
        }
    }
}

impl Piece {
    fn span(self, text: &str, palette: &Palette) -> InlineSpanRef {
        let mut style = self.run.tag.and_then(|tag| palette.style_of(tag));
        if self.problem {
            style = Some(merged(style, palette.problem_style()));
        }
        if self.composing {
            let underline = TextStyle::new().decoration(TextDecoration::UNDERLINE);
            style = Some(merged(style, underline));
        }
        let span = TextSpan::new().text(text.get(self.run.bytes).unwrap_or_default());
        match style {
            Some(style) => span.style(style).into_span(),
            None => span.into_span(),
        }
    }
}

/// `style` with what `over` sets laid over it.
fn merged(style: Option<TextStyle>, over: TextStyle) -> TextStyle {
    match style {
        Some(style) => style.merge(Some(&over)),
        None => over,
    }
}
