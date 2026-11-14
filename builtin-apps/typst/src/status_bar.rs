//! The bar beneath the document: where the caret is, at its start; then how many pages Typst
//! set and how long that took, or that the pages are out of date while the text has an error;
//! and the zoom of the pages at its end. A window showing one side shows that side's part.

use waft::{
    App, BuildContext, EdgeInsetsGeometry, IntoWidget, Listener, Padding, Row, SizedBox, Spacer,
    StatelessWidget, Text, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, TextBlockStyle, ThemeResources, ToolTip, ToolTipService};

use crate::bar_button::icon_button;
use crate::documents_app::DocumentsApp;
use crate::icons;
use crate::pages::Pages;
use crate::viewing::Side;
use crate::zoom::Zoom;
use crate::zooming::{toggle_fit, zoom_in, zoom_out};

#[derive(Debug)]
pub(crate) struct StatusBar {
    pub(crate) documents: DocumentsApp,
    /// The one side the window shows, where it is too narrow for both.
    pub(crate) only: Option<Side>,
}

impl StatelessWidget for StatusBar {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let mut parts = Vec::new();
        if self.only != Some(Side::Preview) {
            let (line, column) = *self.documents.caret.read(app);
            parts.push(said(app, context, format!("Line {line}, Column {column}")));
        }
        parts.push(Spacer::new().into_widget());
        if self.only != Some(Side::Write) {
            let pages = said(app, context, pages_said(self.documents.pages.read(app)));
            parts.push(pages);
            parts.push(SizedBox::new().width(12.0).into_widget());
            parts.extend(self.zoom_controls(app));
        }
        Padding::new(EdgeInsetsGeometry::from_ltrb(16.0, 2.0, 8.0, 2.0))
            .child(
                SizedBox::new()
                    .height(32.0)
                    .child(Row::new().children(parts)),
            )
            .into_widget()
    }
}

impl StatusBar {
    /// Zoom Out, the zoom shown, which fits the width or shows the pages' own size, and Zoom In.
    fn zoom_controls(&self, app: &App) -> [WidgetRef; 3] {
        let documents = &self.documents;
        let label = match documents.viewing.read(app).zoom {
            Zoom::FitWidth => "Fit".to_owned(),
            Zoom::Percent(percent) => format!("{percent}%"),
        };
        let toggling = documents.clone();
        let toggle = Button::text(
            label,
            Listener::new(move |app: &mut App| toggle_fit(app, &toggling)),
        )
        .style(ButtonStyle::Subtle);
        [
            icon_button(icons::ZOOM_OUT, "Zoom Out", documents, zoom_out),
            ToolTipService::new(toggle, ToolTip::text("Fit Width or Actual Size")).into_widget(),
            icon_button(icons::ZOOM_IN, "Zoom In", documents, zoom_in),
        ]
    }
}

/// How many pages are set and how long that took; that they are out of date while the text has
/// an error.
fn pages_said(pages: &Pages) -> String {
    let errors = pages.error_count();
    if errors > 0 {
        let problems = if errors == 1 {
            "1 error"
        } else {
            &format!("{errors} errors")
        };
        return if pages.count() == 0 {
            problems.to_owned()
        } else {
            format!("{problems} · pages not updated")
        };
    }
    let count = match pages.count() {
        1 => "1 page".to_owned(),
        count => format!("{count} pages"),
    };
    format!("{count} · set in {} ms", pages.took().as_millis().max(1))
}

fn said(app: &mut App, context: BuildContext, text: String) -> WidgetRef {
    let resources = ThemeResources::of(app, context);
    let style = TextBlockStyle::Caption.text_style(&resources, resources.text_fill_color_secondary);
    Text::new(text).style(style).max_lines(1).into_widget()
}
