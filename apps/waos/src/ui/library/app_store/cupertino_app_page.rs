//! An app's page in the app store at a phone's size, in Cupertino's widgets, as the App Store
//! shows an app: Back to the store, named after it, then the app's icon beside its name, who
//! publishes it and what can be done with it: Get or Update, Installing… while it is, and Open
//! once it is installed. Under them what it is in a line, its screenshots in a strip that
//! scrolls sideways, what its source says of it at length, and what else is known of it, as an
//! inset list of labels and values.
//!
//! What the page shows is the library's, read through the store's shared page (`app_page`).

use std::sync::Arc;

use waft::ui::FontWeight;
use waft::{
    App, Axis, BorderRadiusGeometry, BoxDecoration, BoxFit, BuildContext, ClipRRect, Column,
    CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Entity, Expanded, Image, IntoWidget,
    ListView, Listener, MainAxisAlignment, Padding, Row, SingleChildScrollView, SizedBox,
    StatelessWidget, Text, TextOverflow, TextStyle, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoButtonSize, CupertinoColors, CupertinoListSection, CupertinoListTile,
    CupertinoNavigationBar, CupertinoPageScaffold, ObstructingPreferredSizeWidgetRef,
};
use waos_package::ProgramId;

use super::{AppPage, RowAction, StoreApp, app_page};
use crate::state::library::Library;
use crate::ui::library::cupertino::{back_to_page_beneath, under_bar};
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The page's margins, as the App Store's.
const SIDE_MARGIN: f64 = 20.0;

/// The side of the app's icon at the page's top.
const ICON_SIDE: f64 = 96.0;

/// How tall the strip of screenshots is, how round a screenshot's corners are, and how wide one
/// not read yet is taken to be, as a share of that height: a phone's screen, held upright.
const SCREENSHOT_HEIGHT: f64 = 280.0;
const SCREENSHOT_CORNER_RADIUS: f64 = 14.0;
const UNREAD_SCREENSHOT_SHAPE: f64 = 0.46;

/// The room between two parts of the page.
const PART_GAP: f64 = 20.0;

/// The page of the app `id`, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoAppPage {
    pub(crate) library: Entity<Library>,
    pub(crate) id: ProgramId,
}

impl StatelessWidget for CupertinoAppPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let shown = Shown {
            library: self.library.clone(),
            secondary: resolved(CupertinoColors::SECONDARY_LABEL, app, context),
            unread: resolved(CupertinoColors::TERTIARY_SYSTEM_FILL, app, context),
        };
        let parts = match app_page(self.library.read(app), &self.id) {
            Some(page) => shown.parts(&page),
            None => vec![shown.aside(Text::new(words::NO_LONGER_LISTED).into_widget())],
        };
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_page_beneath(&self.library, words::GET_PROGRAMS));
        CupertinoPageScaffold::new(under_bar(move |top| {
            ListView::new()
                .padding(EdgeInsetsGeometry::from_ltrb(0.0, top + 12.0, 0.0, 24.0))
                .children(parts.clone())
                .into_widget()
        }))
        .navigation_bar(ObstructingPreferredSizeWidgetRef::new(bar))
        .background_color(CupertinoColors::SYSTEM_GROUPED_BACKGROUND)
        .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Shown {
    library: Entity<Library>,
    /// The colour of what the page says beside its main text.
    secondary: waft::Color,
    /// The colour of the frame a screenshot not read yet keeps.
    unread: waft::Color,
}

impl Shown {
    /// The page's parts, the first to the last, each with the room under it.
    fn parts(&self, page: &AppPage) -> Vec<WidgetRef> {
        let mut parts = vec![self.aside(self.header(page))];
        if let Some(summary) = &page.summary {
            parts.push(self.aside(Text::new(summary.as_str()).into_widget()));
        }
        if !page.screenshots.is_empty() {
            parts.push(self.screenshots(&page.screenshots));
        }
        if !page.description.is_empty() {
            parts.push(self.aside(self.description(&page.description)));
        }
        let under = |part: WidgetRef| {
            Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, PART_GAP))
                .child(part)
                .into_widget()
        };
        let mut parts: Vec<WidgetRef> = parts.into_iter().map(under).collect();
        parts.push(self.details(&page.details));
        parts
    }

    /// `part` in from the page's sides.
    fn aside(&self, part: WidgetRef) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::symmetric(0.0, SIDE_MARGIN))
            .child(part)
            .into_widget()
    }

    /// The app's icon, and beside it its name over who publishes it, with what can be done with
    /// it at the foot, as the App Store's page sets them.
    fn header(&self, page: &AppPage) -> WidgetRef {
        let icon = ProgramIcon {
            png: page.app.icon.clone(),
            side: ICON_SIDE,
        };
        let name = Text::new(page.app.app.listed.name.as_str())
            .max_lines(2)
            .overflow(TextOverflow::Ellipsis)
            .style(
                TextStyle::new()
                    .font_size(22.0)
                    .font_weight(FontWeight::W700),
            );
        let mut named = vec![name.into_widget()];
        if let Some(publisher) = &page.publisher {
            named.push(self.said(publisher));
        }
        let beside = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .children([
                Column::new()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .children(named)
                    .into_widget(),
                self.action(&page.app),
            ]);
        Row::new()
            .spacing(16.0)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([
                icon.into_widget(),
                Expanded::new(SizedBox::new().height(ICON_SIDE).child(beside)).into_widget(),
            ])
            .into_widget()
    }

    /// What can be done with the app: Get or Update, which install what its source lists;
    /// Installing… while that goes on; Open once it is installed.
    fn action(&self, app: &StoreApp) -> WidgetRef {
        let (library, listed) = (self.library.clone(), app.app.clone());
        let button = match app.action {
            RowAction::Installing => return self.said(words::INSTALLING),
            RowAction::Installed => CupertinoButton::tinted(
                Text::new(words::OPEN).into_widget(),
                Some(Listener::new(move |ui| {
                    library.update(ui, |library, cx| library.open(&listed.listed.id, cx));
                })),
            ),
            RowAction::Get | RowAction::Update => {
                let label = if app.action == RowAction::Get {
                    words::GET
                } else {
                    words::UPDATE
                };
                CupertinoButton::filled(
                    Text::new(label).into_widget(),
                    Some(Listener::new(move |ui| {
                        library.update(ui, |library, cx| {
                            library.install_from(&listed.source, &listed.listed, cx);
                        });
                    })),
                )
            }
        };
        button.size_style(CupertinoButtonSize::Small).into_widget()
    }

    /// The screenshots side by side, in a strip that scrolls sideways; one not read yet keeps
    /// its place as an empty frame.
    fn screenshots(&self, screenshots: &[Option<Arc<[u8]>>]) -> WidgetRef {
        let corners = BorderRadiusGeometry::circular(SCREENSHOT_CORNER_RADIUS);
        let frames: Vec<WidgetRef> = screenshots
            .iter()
            .map(|screenshot| match screenshot {
                Some(png) => ClipRRect::new()
                    .border_radius(corners)
                    .child(
                        Image::memory(png.clone())
                            .height(SCREENSHOT_HEIGHT)
                            .fit(BoxFit::FitHeight),
                    )
                    .into_widget(),
                None => SizedBox::new()
                    .width(SCREENSHOT_HEIGHT * UNREAD_SCREENSHOT_SHAPE)
                    .height(SCREENSHOT_HEIGHT)
                    .child(DecoratedBox::new(BoxDecoration {
                        color: Some(self.unread.into()),
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
                    .padding(EdgeInsetsGeometry::symmetric(0.0, SIDE_MARGIN))
                    .child(Row::new().spacing(12.0).children(frames)),
            )
            .into_widget()
    }

    /// What the app's source says of it at length, a paragraph under a paragraph.
    fn description(&self, paragraphs: &[String]) -> WidgetRef {
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(8.0)
            .children(
                paragraphs
                    .iter()
                    .map(|paragraph| Text::new(paragraph.as_str()).into_widget()),
            )
            .into_widget()
    }

    /// What else is known of the app, as an inset list: each label with its value at its end.
    fn details(&self, details: &[(&'static str, String)]) -> WidgetRef {
        let tiles: Vec<WidgetRef> = details
            .iter()
            .map(|(label, value)| {
                CupertinoListTile::new(Text::new(*label))
                    .additional_info(Text::new(value.as_str()))
                    .into_widget()
            })
            .collect();
        CupertinoListSection::inset_grouped()
            .children(tiles)
            .into_widget()
    }

    /// `text` in the secondary colour.
    fn said(&self, text: &str) -> WidgetRef {
        Text::new(text)
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(TextStyle::new().font_size(15.0).color(self.secondary))
            .into_widget()
    }
}
