//! The bar above the document: New, Open and Save at its start, Export PDF at its end, and
//! between them the document's name, or, in a window too narrow for the text beside its pages,
//! the selector of which of the two it shows.

use waft::{
    App, BuildContext, Center, EdgeInsetsGeometry, Expanded, IntoWidget, Listener, MainAxisSize,
    Padding, Row, SizedBox, StatelessWidget, Text, TextOverflow, WidgetRef,
};
use waft_winui::{Button, SelectorBar, SelectorBarItem, TextBlockStyle, ThemeResources};

use crate::bar_button::icon_button;
use crate::documents_app::DocumentsApp;
use crate::exporting::export_pdf;
use crate::filing::{new_document, open_document, save_document};
use crate::icons::{self, icon};
use crate::viewing::Side;
use crate::zooming::{show_pages, show_text};

#[derive(Debug)]
pub(crate) struct CommandBar {
    pub(crate) documents: DocumentsApp,
    /// Whether the window shows one side at a time.
    pub(crate) narrow: bool,
}

impl StatelessWidget for CommandBar {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let documents = &self.documents;
        let middle = if self.narrow {
            self.side_selector(app)
        } else {
            self.name(app, context)
        };
        let parts = vec![
            icon_button(icons::NEW, "New", documents, new_document),
            icon_button(icons::OPEN, "Open…", documents, open_document),
            icon_button(icons::SAVE, "Save", documents, save_document),
            Expanded::new(Center::new().child(middle)).into_widget(),
            self.export_button(),
        ];
        Padding::new(EdgeInsetsGeometry::from_ltrb(8.0, 6.0, 8.0, 6.0))
            .child(Row::new().children(parts))
            .into_widget()
    }
}

impl CommandBar {
    /// The document's name, and that it is edited while it is.
    fn name(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let document = self.documents.document.read(app);
        let name = if document.is_edited() {
            format!("{} — Edited", document.name())
        } else {
            document.name()
        };
        let style =
            TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary);
        Text::new(name)
            .style(style)
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .into_widget()
    }

    /// Write or Preview, the side shown selected.
    fn side_selector(&self, app: &mut App) -> WidgetRef {
        let items = vec![
            SelectorBarItem::new("write", "Write"),
            SelectorBarItem::new("preview", "Preview"),
        ];
        let selected = match self.documents.viewing.read(app).side {
            Side::Write => 0,
            Side::Preview => 1,
        };
        let documents = self.documents.clone();
        SelectorBar::new(items, Some(selected), move |app, selected| match selected {
            Some(1) => show_pages(app, &documents),
            _ => show_text(app, &documents),
        })
        .into_widget()
    }

    /// Export PDF, by its name where there is room for it.
    fn export_button(&self) -> WidgetRef {
        if self.narrow {
            return icon_button(icons::PDF, "Export PDF…", &self.documents, export_pdf);
        }
        let documents = self.documents.clone();
        let label = Row::new().main_axis_size(MainAxisSize::Min).children([
            icon(icons::PDF).into_widget(),
            SizedBox::new().width(8.0).into_widget(),
            Text::new("Export PDF").into_widget(),
        ]);
        Button::new(
            label,
            Listener::new(move |app: &mut App| export_pdf(app, &documents)),
        )
        .into_widget()
    }
}
