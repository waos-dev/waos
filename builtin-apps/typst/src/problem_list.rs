//! The problems Typst found in the text, beneath the editor: each an error or a warning, what
//! it is, its line and column, and Typst's hints. A click on one selects its place in the text.
//! The list is as high as its problems up to [`MAX_HEIGHT`], and scrolls past that.

use waft::ui::TextSelection;
use waft::{
    App, BoxConstraints, BuildContext, ColoredBox, Column, ConstrainedBox, CrossAxisAlignment,
    EdgeInsetsGeometry, Expanded, FocusNodeLeaf, GestureDetector, IntoWidget, Listener,
    MainAxisSize, Padding, Row, SingleChildScrollView, SizedBox, StatelessWidget, Text,
    TextEditingControllerLeaf, WidgetRef,
};
use waft_rendering::HitTestBehavior;
use waft_winui::{TextBlockStyle, ThemeResources};

use crate::documents_app::DocumentsApp;
use crate::icons::{self, icon};
use crate::typesetting::{Place, Problem};

/// The most room the list takes of the editor's, in logical points.
const MAX_HEIGHT: f64 = 148.0;

#[derive(Debug)]
pub(crate) struct ProblemList {
    pub(crate) documents: DocumentsApp,
}

impl StatelessWidget for ProblemList {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let problems = self.documents.pages.read(app).problems().to_vec();
        if problems.is_empty() {
            return SizedBox::new().into_widget();
        }
        let resources = ThemeResources::of(app, context);
        let rows: Vec<WidgetRef> = problems
            .into_iter()
            .map(|problem| self.row(&resources, problem))
            .collect();
        let list = SingleChildScrollView::new().child(
            Column::new()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children(rows),
        );
        let bounded = ConstrainedBox::new(BoxConstraints {
            max_height: MAX_HEIGHT,
            ..BoxConstraints::default()
        })
        .child(Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 4.0, 0.0, 4.0)).child(list));
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                line(resources.divider_stroke_color_default),
                bounded.into_widget(),
            ])
            .into_widget()
    }
}

impl ProblemList {
    /// A problem: its icon, what it is, and beneath, where it is and Typst's hints.
    fn row(&self, resources: &ThemeResources, problem: Problem) -> WidgetRef {
        let (glyph, colour) = if problem.is_error {
            (icons::ERROR, resources.system_fill_color_critical)
        } else {
            (icons::WARNING, resources.system_fill_color_caution)
        };
        let message = Text::new(problem.message.clone())
            .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary));
        let detail = Text::new(detail_of(&problem)).style(
            TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary),
        );
        let said = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([message.into_widget(), detail.into_widget()]);
        let row = Row::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([
                Padding::new(EdgeInsetsGeometry::from_ltrb(0.0, 2.0, 10.0, 0.0))
                    .child(icon(glyph).foreground(colour))
                    .into_widget(),
                Expanded::new(said).into_widget(),
            ]);
        let documents = self.documents.clone();
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(Listener::new(move |app: &mut App| {
                go_to(app, &documents, &problem)
            }))
            .child(Padding::new(EdgeInsetsGeometry::from_ltrb(16.0, 4.0, 16.0, 4.0)).child(row))
            .into_widget()
    }
}

/// Where the problem is, then Typst's hints.
fn detail_of(problem: &Problem) -> String {
    let place = match &problem.place {
        Place::Text { line, column, .. } => format!("Line {line}, column {column}"),
        Place::File(name) => format!("In {name}"),
        Place::Unknown => String::new(),
    };
    let hints = problem.hints.iter().map(|hint| format!("Hint: {hint}"));
    let parts: Vec<String> = std::iter::once(place)
        .chain(hints)
        .filter(|part| !part.is_empty())
        .collect();
    parts.join(" · ")
}

/// Selects the problem's place in the text, with the focus in the editor.
fn go_to(app: &mut App, documents: &DocumentsApp, problem: &Problem) {
    let Place::Text { utf16, .. } = &problem.place else {
        return;
    };
    let selection = TextSelection::new(utf16.start as i32, utf16.end as i32);
    documents.editor.text.set_selection(app, selection);
    documents.editor.focus.request_focus(app, None);
}

/// A hairline across the pane.
pub(crate) fn line(colour: waft::Color) -> WidgetRef {
    SizedBox::new()
        .height(1.0)
        .child(ColoredBox::new(colour))
        .into_widget()
}
