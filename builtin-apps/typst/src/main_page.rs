//! The window's page, clear of a phone's bars and keyboard: the command bar, what the window says, the document, and the status bar.
//! From [`BOTH_FROM_WIDTH`] logical points wide the document is its text beside its pages, as on
//! a desktop or a tablet; narrower, as on a phone or in a narrow window, it is one of the two,
//! which the command bar's selector switches. The line is where each side would be narrower
//! than a phone.

use waft::{
    App, BuildContext, ColoredBox, Column, CrossAxisAlignment, Expanded, IntoWidget, LayoutBuilder,
    Row, SizedBox, StatelessWidget, WidgetRef,
};
use waft_winui::{Brush, Page, ThemeResources};

use crate::command_bar::CommandBar;
use crate::documents_app::DocumentsApp;
use crate::editor_pane::EditorPane;
use crate::folder_access::FolderBar;
use crate::notices::NoticeBar;
use crate::preview_pane::PreviewPane;
use crate::problem_list::line;
use crate::status_bar::StatusBar;
use crate::viewing::Side;

/// The width from which the text and the pages are side by side, in logical points.
const BOTH_FROM_WIDTH: f64 = 720.0;

#[derive(Debug)]
pub(crate) struct MainPage {
    pub(crate) documents: DocumentsApp,
}

impl StatelessWidget for MainPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let background = ThemeResources::of(app, context).solid_background_fill_color_base;
        let documents = self.documents.clone();
        let side = documents.viewing.read(app).side;
        let page = LayoutBuilder::new(move |app, context, constraints| {
            let only = (constraints.max_width < BOTH_FROM_WIDTH).then_some(side);
            laid_out(app, context, &documents, only)
        });
        Page::new()
            .background(Brush::Solid(background))
            .content(page.into_widget())
            .into_widget()
    }
}

/// The page's parts, one under another, the document showing `only` one side where the window
/// is too narrow for both.
fn laid_out(
    app: &mut App,
    context: BuildContext,
    documents: &DocumentsApp,
    only: Option<Side>,
) -> WidgetRef {
    let hairline = ThemeResources::of(app, context).divider_stroke_color_default;
    let document = match only {
        Some(Side::Write) => editor(documents),
        Some(Side::Preview) => preview(documents),
        None => Row::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Expanded::new(editor(documents)).into_widget(),
                SizedBox::new()
                    .width(1.0)
                    .child(ColoredBox::new(hairline))
                    .into_widget(),
                Expanded::new(preview(documents)).into_widget(),
            ])
            .into_widget(),
    };
    let documents = documents.clone();
    Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children([
            CommandBar {
                documents: documents.clone(),
                narrow: only.is_some(),
            }
            .into_widget(),
            NoticeBar {
                documents: documents.clone(),
            }
            .into_widget(),
            FolderBar {
                documents: documents.clone(),
            }
            .into_widget(),
            line(hairline),
            Expanded::new(document).into_widget(),
            line(hairline),
            StatusBar { documents, only }.into_widget(),
        ])
        .into_widget()
}

fn editor(documents: &DocumentsApp) -> WidgetRef {
    EditorPane {
        documents: documents.clone(),
    }
    .into_widget()
}

fn preview(documents: &DocumentsApp) -> WidgetRef {
    PreviewPane {
        documents: documents.clone(),
    }
    .into_widget()
}
