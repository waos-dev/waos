//! An app's page in the app store, in the desktop's panel, in Fluent's controls: Back, then the
//! app's icon, its name, who publishes it and what it is in a line, beside what can be done with
//! it: Get or Update, Installing… while it is, and Open once it is installed. Under them its
//! screenshots, side by side in a strip that scrolls, what its source says of it at length, and
//! what else is known of it, each under its label.
//!
//! What the page shows is the library's, read through the store's shared page (`app_page`).

use std::rc::Rc;
use std::sync::Arc;

use waft::{
    App, Axis, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration, BoxFit,
    BuildContext, ClipRRect, Column, CrossAxisAlignment, DecoratedBox, DecorationPosition,
    EdgeInsetsGeometry, Entity, Expanded, Image, IntoWidget, ListView, Listener, Padding, Row,
    SingleChildScrollView, SizedBox, StatelessWidget, Text, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, FluentSymbol, FontIcon, TextBlockStyle, ThemeResources};
use waos_package::ProgramId;

use super::{AppPage, RowAction, StoreApp, app_page};
use crate::state::library::Library;
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The side of the app's icon at the page's top.
pub(crate) const ICON_SIDE: f64 = 72.0;

/// How tall the strip of screenshots is, how round a screenshot's corners are, and how wide one
/// not read yet is taken to be, as a share of that height.
const SCREENSHOT_HEIGHT: f64 = 200.0;
const SCREENSHOT_CORNER_RADIUS: f64 = 8.0;
const UNREAD_SCREENSHOT_SHAPE: f64 = 1.6;

/// The room between two parts of the page, and between two lines of one part.
const PART_GAP: f64 = 20.0;
const LINE_GAP: f64 = 8.0;

/// How wide the labels of what else is known of the app are, so their values line up.
const LABEL_WIDTH: f64 = 96.0;

/// The page of the app `id`, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentAppPage {
    pub(crate) library: Entity<Library>,
    pub(crate) id: ProgramId,
}

impl StatelessWidget for FluentAppPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let shown = Shown {
            library: self.library.clone(),
            resources: ThemeResources::of(app, context),
        };
        let parts = match app_page(self.library.read(app), &self.id) {
            Some(page) => shown.parts(page),
            None => vec![shown.said(words::NO_LONGER_LISTED)],
        };
        let list = ListView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 8.0, MARGIN, MARGIN))
            .children(parts);
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([shown.back(), Expanded::new(list).into_widget()])
            .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Shown {
    library: Entity<Library>,
    resources: Rc<ThemeResources>,
}

impl Shown {
    /// Back to the app store.
    fn back(&self) -> WidgetRef {
        let library = self.library.clone();
        let back = Listener::new(move |app| {
            library.update(app, |library, cx| library.go_back(cx));
        });
        let button = Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
            .style(ButtonStyle::Subtle)
            .padding([8.0, 8.0, 8.0, 8.0]);
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN - 8.0,
            16.0,
            MARGIN,
            4.0,
        ))
        .child(Row::new().children([button.into_widget()]))
        .into_widget()
    }

    /// The page's parts, the first to the last, each with the room under it.
    fn parts(&self, page: AppPage) -> Vec<WidgetRef> {
        let mut parts = vec![self.header(&page)];
        if !page.screenshots.is_empty() {
            parts.push(self.screenshots(&page.screenshots));
        }
        if !page.description.is_empty() {
            parts.push(self.description(&page.description));
        }
        parts.push(self.details(&page.details));
        parts
            .into_iter()
            .map(|part| {
                Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, PART_GAP))
                    .child(part)
                    .into_widget()
            })
            .collect()
    }

    /// The app's icon, its name over who publishes it and what it is, and what can be done with
    /// it.
    fn header(&self, page: &AppPage) -> WidgetRef {
        let icon = ProgramIcon {
            png: page.app.icon.clone(),
            side: ICON_SIDE,
        };
        let mut lines = vec![self.line(&page.app.app.listed.name, TextBlockStyle::Subtitle)];
        if let Some(publisher) = &page.publisher {
            lines.push(self.said(publisher));
        }
        if let Some(summary) = &page.summary {
            lines.push(
                Padding::new(EdgeInsetsGeometry::only(0.0, 4.0, 0.0, 0.0))
                    .child(self.line(summary, TextBlockStyle::Body))
                    .into_widget(),
            );
        }
        let text = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(lines);
        Row::new()
            .spacing(16.0)
            .children([
                icon.into_widget(),
                Expanded::new(text).into_widget(),
                self.action(&page.app),
            ])
            .into_widget()
    }

    /// What can be done with the app: Get or Update, which install what its source lists;
    /// Installing… while that goes on; Open once it is installed.
    fn action(&self, app: &StoreApp) -> WidgetRef {
        let (library, listed) = (self.library.clone(), app.app.clone());
        let (label, style, does): (_, _, Listener) = match app.action {
            RowAction::Installing => return self.said(words::INSTALLING),
            RowAction::Installed => (
                words::OPEN,
                ButtonStyle::Default,
                Listener::new(move |ui| {
                    library.update(ui, |library, cx| library.open(&listed.listed.id, cx));
                }),
            ),
            RowAction::Get | RowAction::Update => (
                if app.action == RowAction::Get {
                    words::GET
                } else {
                    words::UPDATE
                },
                ButtonStyle::Accent,
                Listener::new(move |ui| {
                    library.update(ui, |library, cx| {
                        library.install_from(&listed.source, &listed.listed, cx);
                    });
                }),
            ),
        };
        Button::text(label, does).style(style).into_widget()
    }

    /// The screenshots side by side, in a strip that scrolls sideways where they outgrow the
    /// page; one not read yet keeps its place as an empty frame.
    fn screenshots(&self, screenshots: &[Option<Arc<[u8]>>]) -> WidgetRef {
        let corners = BorderRadiusGeometry::circular(SCREENSHOT_CORNER_RADIUS);
        // A hairline along a screenshot's edge keeps a light one apart from a light page.
        let edge = BoxDecoration {
            border: Some(
                Border::all(
                    self.resources.card_stroke_color_default,
                    1.0,
                    BorderStyle::Solid,
                    BorderSide::STROKE_ALIGN_INSIDE,
                )
                .into(),
            ),
            border_radius: Some(corners),
            ..BoxDecoration::default()
        };
        let frames: Vec<WidgetRef> = screenshots
            .iter()
            .map(|screenshot| match screenshot {
                Some(png) => DecoratedBox::new(edge.clone())
                    .position(DecorationPosition::Foreground)
                    .child(
                        ClipRRect::new().border_radius(corners).child(
                            Image::memory(png.clone())
                                .height(SCREENSHOT_HEIGHT)
                                .fit(BoxFit::FitHeight),
                        ),
                    )
                    .into_widget(),
                None => SizedBox::new()
                    .width(SCREENSHOT_HEIGHT * UNREAD_SCREENSHOT_SHAPE)
                    .height(SCREENSHOT_HEIGHT)
                    .child(DecoratedBox::new(BoxDecoration {
                        color: Some(self.resources.subtle_fill_color_secondary.into()),
                        border_radius: Some(corners),
                        ..BoxDecoration::default()
                    }))
                    .into_widget(),
            })
            .collect();
        SizedBox::new()
            .height(SCREENSHOT_HEIGHT)
            .child(
                SingleChildScrollView::new()
                    .scroll_direction(Axis::Horizontal)
                    .child(Row::new().spacing(12.0).children(frames)),
            )
            .into_widget()
    }

    /// What the app's source says of it at length, a paragraph under a paragraph.
    fn description(&self, paragraphs: &[String]) -> WidgetRef {
        let resources = &self.resources;
        let style = TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary);
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(LINE_GAP)
            .children(paragraphs.iter().map(|paragraph| {
                Text::new(paragraph.as_str())
                    .style(style.clone())
                    .into_widget()
            }))
            .into_widget()
    }

    /// What else is known of the app, each under its label, the values in one column.
    fn details(&self, details: &[(&'static str, String)]) -> WidgetRef {
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(LINE_GAP)
            .children(details.iter().map(|(label, value)| {
                Row::new()
                    .children([
                        SizedBox::new()
                            .width(LABEL_WIDTH)
                            .child(self.said(label))
                            .into_widget(),
                        Expanded::new(self.line(value, TextBlockStyle::Body)).into_widget(),
                    ])
                    .into_widget()
            }))
            .into_widget()
    }

    /// `text` on one line, in `style` and the primary colour.
    fn line(&self, text: &str, style: TextBlockStyle) -> WidgetRef {
        let resources = &self.resources;
        Text::new(text)
            .max_lines(1)
            .overflow(waft::TextOverflow::Ellipsis)
            .style(style.text_style(resources, resources.text_fill_color_primary))
            .into_widget()
    }

    /// `text` in the secondary colour: a label, or what the page says of itself.
    fn said(&self, text: &str) -> WidgetRef {
        let resources = &self.resources;
        Text::new(text)
            .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary))
            .into_widget()
    }
}
