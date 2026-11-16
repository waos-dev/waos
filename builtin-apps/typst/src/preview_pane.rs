//! The side the pages are shown on: one under another on a grey desk, as a PDF viewer shows
//! them, fitted to the pane's width or at the zoom chosen, scrolled down through the pages and,
//! where they are wider than the pane, across them. Only the pages in sight are built, and so
//! drawn. Until Typst has set any, the pane says why there is none.

use std::rc::Rc;

use waft::{
    App, Axis, BuildContext, Center, ColoredBox, EdgeInsetsGeometry, IntoWidget, LayoutBuilder,
    ListView, Padding, SingleChildScrollView, SizedBox, StatelessWidget, Text, TextAlign,
    WidgetRef,
};
use waft_winui::{TextBlockStyle, ThemeResources};

use crate::documents_app::DocumentsApp;
use crate::page_sheet::{HALF_GAP, PageSheet};
use crate::zoom::{PAGE_MARGIN, Zoom};

#[derive(Debug)]
pub(crate) struct PreviewPane {
    pub(crate) documents: DocumentsApp,
}

impl StatelessWidget for PreviewPane {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let desk = ThemeResources::of(app, context).solid_background_fill_color_secondary;
        let pages = self.documents.pages.read(app);
        if pages.count() == 0 {
            let said = nothing_set(app, context, pages.error_count() > 0);
            return ColoredBox::new(desk).child(said).into_widget();
        }
        let laid_out = LaidOut {
            heights: pages.heights(),
            widest: pages.widest(),
            zoom: self.documents.viewing.read(app).zoom,
        };
        let documents = self.documents.clone();
        let pages = LayoutBuilder::new(move |app, _context, constraints| {
            pages_in(app, &documents, laid_out.clone(), constraints.max_width)
        });
        ColoredBox::new(desk)
            .child(pages.into_widget())
            .into_widget()
    }
}

/// What the pages' layout is made from, read as the pane is built.
#[derive(Clone)]
struct LaidOut {
    /// Each page's height, in points.
    heights: Rc<[f64]>,
    /// The widest page's width, in points.
    widest: f64,
    zoom: Zoom,
}

/// The pages, in a pane `width` logical points wide. Each page's room in the list is told from
/// its height, so scrolling far builds, and so draws, only the pages it arrives at.
fn pages_in(app: &mut App, documents: &DocumentsApp, laid_out: LaidOut, width: f64) -> WidgetRef {
    let LaidOut {
        heights,
        widest,
        zoom,
    } = laid_out;
    let scale = zoom.scale(width, widest);
    // Not a change the window shows: only what zooming steps from.
    documents
        .viewing
        .update(app, |viewing, _cx| viewing.laid_out = (width, widest));
    let sheets = documents.clone();
    let count = heights.len();
    let list = ListView::builder(move |_app, _context, index| {
        let sheet = PageSheet {
            documents: sheets.clone(),
            index: index as usize,
            scale,
        };
        Some(sheet.into_widget())
    })
    .item_count(count as i32)
    .build()
    .controller(documents.preview_scroll.erase())
    .item_extent_builder(Rc::new(move |index, _dimensions| {
        let height = heights.get(usize::try_from(index).ok()?)?;
        Some(height * scale + 2.0 * HALF_GAP)
    }));
    // Always in the sideways scroller, wider than the pane or not, so the list stays the same
    // widget, and keeps its place, when zooming makes the pages wider than the pane.
    let needed = (widest * scale + 2.0 * PAGE_MARGIN).max(width);
    SingleChildScrollView::new()
        .scroll_direction(Axis::Horizontal)
        .child(SizedBox::new().width(needed).child(list))
        .into_widget()
}

/// What the pane says while there is no page: that the text has problems, or nothing.
fn nothing_set(app: &mut App, context: BuildContext, has_errors: bool) -> WidgetRef {
    if !has_errors {
        return SizedBox::new().into_widget();
    }
    let resources = ThemeResources::of(app, context);
    let style = TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary);
    let said = Text::new("The pages appear here once the problems in the text are fixed.")
        .style(style)
        .text_align(TextAlign::Center);
    Center::new()
        .child(Padding::new(EdgeInsetsGeometry::all(24.0)).child(said))
        .into_widget()
}
