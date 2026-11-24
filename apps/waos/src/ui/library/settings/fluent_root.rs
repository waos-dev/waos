//! The settings' first page at a desktop's width, in Fluent's controls, as Windows's Settings
//! leads to its pages: Back to the home and the page's title, then a card for each place it
//! leads to, its mark, its name and what it holds, and a chevron: WaOS's own settings, where
//! this system has any; the settings every app follows; and, under Apps, each installed app.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use std::rc::Rc;

use waft::{
    App, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration, BuildContext,
    Center, Column, CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Entity, Expanded,
    IntoWidget, ListView, Listener, Padding, Row, SizedBox, StatelessWidget, Text, TextOverflow,
    WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, CommonState, FluentSymbol, FontIcon, TextBlockStyle, ThemeResources,
};

use super::{listed_apps, waos_has_settings};
use crate::state::library::{Library, LibraryPage, SettingsPage};
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The side of what marks a card: an app's icon, or the room a symbol stands in.
const MARK_SIDE: f64 = 32.0;

/// How round a card's corners are: WinUI's control corners.
const CARD_CORNER_RADIUS: f64 = 4.0;

/// The room between two cards.
const CARD_GAP: f64 = 4.0;

/// The settings' first page, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentSettingsRoot {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for FluentSettingsRoot {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let shown = Shown {
            library: self.library.clone(),
            resources: ThemeResources::of(app, context),
        };
        let library = self.library.read(app);
        let mut rows = Vec::new();
        if waos_has_settings(library, app) {
            rows.push(shown.card(
                shown.symbol(FluentSymbol::Settings),
                words::WAOS_SETTINGS,
                Some(words::WAOS_SETTINGS_DETAIL),
                SettingsPage::Waos,
            ));
        }
        rows.push(shown.card(
            shown.symbol(FluentSymbol::Apps),
            words::EVERY_APP,
            Some(words::GLOBAL_DETAIL),
            SettingsPage::EveryApp,
        ));
        let apps = listed_apps(library);
        if !apps.is_empty() {
            rows.push(shown.heading(words::APPS));
        }
        for listed in apps {
            let icon = ProgramIcon {
                png: listed.icon,
                side: MARK_SIDE,
            };
            rows.push(shown.card(
                icon.into_widget(),
                &listed.name,
                None,
                SettingsPage::App(listed.id),
            ));
        }
        let list = ListView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 8.0, MARGIN, MARGIN))
            .children(rows);
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([shown.title_bar(), Expanded::new(list).into_widget()])
            .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Shown {
    library: Entity<Library>,
    resources: Rc<ThemeResources>,
}

impl Shown {
    /// Back to the home, and the page's title.
    fn title_bar(&self) -> WidgetRef {
        let library = self.library.clone();
        let back = Listener::new(move |app| {
            library.update(app, |library, cx| library.go_back(cx));
        });
        let primary = self.resources.text_fill_color_primary;
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN - 8.0,
            16.0,
            MARGIN,
            4.0,
        ))
        .child(
            Row::new().spacing(4.0).children([
                Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
                    .style(ButtonStyle::Subtle)
                    .padding([8.0, 8.0, 8.0, 8.0])
                    .into_widget(),
                Text::new(words::SETTINGS)
                    .style(TextBlockStyle::Subtitle.text_style(&self.resources, primary))
                    .into_widget(),
            ]),
        )
        .into_widget()
    }

    /// A heading over a group of cards.
    fn heading(&self, text: &str) -> WidgetRef {
        let primary = self.resources.text_fill_color_primary;
        Padding::new(EdgeInsetsGeometry::only(0.0, 16.0, 0.0, 8.0))
            .child(
                Text::new(text)
                    .style(TextBlockStyle::BodyStrong.text_style(&self.resources, primary)),
            )
            .into_widget()
    }

    /// `symbol` as a card's mark, in the room an app's icon takes.
    fn symbol(&self, symbol: FluentSymbol) -> WidgetRef {
        let glyph = FontIcon::symbol(symbol)
            .font_size(20.0)
            .foreground(self.resources.text_fill_color_primary);
        SizedBox::new()
            .width(MARK_SIDE)
            .height(MARK_SIDE)
            .child(Center::new().child(glyph))
            .into_widget()
    }

    /// A card that leads to `page`: its `mark`, `name` over what it holds where that is said,
    /// and a chevron; a press anywhere on it opens the page.
    fn card(
        &self,
        mark: WidgetRef,
        name: &str,
        detail: Option<&str>,
        page: SettingsPage,
    ) -> WidgetRef {
        let resources = &self.resources;
        let mut lines = vec![
            Text::new(name)
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis)
                .style(
                    TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary),
                )
                .into_widget(),
        ];
        if let Some(detail) = detail {
            lines.push(
                Text::new(detail)
                    .style(
                        TextBlockStyle::Caption
                            .text_style(resources, resources.text_fill_color_secondary),
                    )
                    .into_widget(),
            );
        }
        let chevron = FontIcon::symbol(FluentSymbol::ChevronRight)
            .font_size(12.0)
            .foreground(resources.text_fill_color_secondary);
        let said = Row::new().spacing(16.0).children([
            mark,
            Expanded::new(
                Column::new()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(lines),
            )
            .into_widget(),
            chevron.into_widget(),
        ]);
        let library = self.library.clone();
        let open = Listener::new(move |app| {
            let page = LibraryPage::Settings(page.clone());
            library.update(app, |library, cx| library.show_page(page, cx));
        });
        // A button for what it does, drawn as the card Windows's Settings leads on with.
        let card = Button::new(said, open).template(|app, context, states, said| {
            let resources = ThemeResources::of(app, context);
            let fill = match states.common {
                CommonState::PointerOver | CommonState::Pressed => {
                    resources.control_fill_color_secondary
                }
                CommonState::Normal | CommonState::Disabled => {
                    resources.card_background_fill_color_default
                }
            };
            let surface = BoxDecoration {
                color: Some(fill.into()),
                border: Some(
                    Border::all(
                        resources.card_stroke_color_default,
                        1.0,
                        BorderStyle::Solid,
                        BorderSide::STROKE_ALIGN_INSIDE,
                    )
                    .into(),
                ),
                border_radius: Some(BorderRadiusGeometry::circular(CARD_CORNER_RADIUS)),
                ..BoxDecoration::default()
            };
            DecoratedBox::new(surface)
                .child(Padding::new(EdgeInsetsGeometry::symmetric(12.0, 16.0)).child(said))
                .into_widget()
        });
        Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, CARD_GAP))
            .child(card)
            .into_widget()
    }
}
