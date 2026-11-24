//! The settings' pages that lead to others, at a desktop's width, in Fluent's controls, as
//! Windows's Settings leads to its pages: a card for each place, its mark, its name and what it
//! holds, and a chevron. The first page leads to General, where this system has settings of
//! WaOS's own, to Permissions, and, under Apps, to each installed app; Permissions leads to
//! each kind, saying what its default is.
//!
//! What they show is the library's, read as they build; they keep nothing of their own.

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

use super::{listed_apps, permission_defaults, waos_has_settings};
use crate::logic::permissions::Permission;
use crate::state::library::{Library, LibraryPage, SettingsPage};
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The side of what marks a card: an app's icon, or the room a symbol stands in.
pub(crate) const MARK_SIDE: f64 = 32.0;

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
        let resources = ThemeResources::of(app, context);
        let library = self.library.read(app);
        let mut rows = Vec::new();
        if waos_has_settings(library, app) {
            rows.push(link_card(
                &self.library,
                &resources,
                Some(symbol(FluentSymbol::Settings, &resources)),
                words::WAOS_SETTINGS,
                Some(words::WAOS_SETTINGS_DETAIL),
                SettingsPage::General,
            ));
        }
        rows.push(link_card(
            &self.library,
            &resources,
            Some(symbol(FluentSymbol::Shield, &resources)),
            words::PERMISSIONS,
            Some(words::PERMISSIONS_DETAIL),
            SettingsPage::Permissions,
        ));
        let apps = listed_apps(library);
        if !apps.is_empty() {
            rows.push(heading(words::APPS, &resources));
        }
        for listed in apps {
            let icon = ProgramIcon {
                png: listed.icon,
                side: MARK_SIDE,
            };
            rows.push(link_card(
                &self.library,
                &resources,
                Some(icon.into_widget()),
                &listed.name,
                None,
                SettingsPage::App(listed.id),
            ));
        }
        titled(&self.library, &resources, words::SETTINGS, rows)
    }
}

/// The list of permissions, each leading to its page, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentPermissions {
    pub(crate) library: Entity<Library>,
}

impl StatelessWidget for FluentPermissions {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let rows = permission_defaults(self.library.read(app))
            .into_iter()
            .map(|(permission, default)| {
                link_card(
                    &self.library,
                    &resources,
                    Some(symbol(permission_symbol(permission), &resources)),
                    words::permission_name(permission),
                    Some(&default),
                    SettingsPage::Permission(permission),
                )
            })
            .collect();
        titled(&self.library, &resources, words::PERMISSIONS, rows)
    }
}

/// A page of `rows` under Back and its `title`.
fn titled(
    library: &Entity<Library>,
    resources: &Rc<ThemeResources>,
    title: &str,
    rows: Vec<WidgetRef>,
) -> WidgetRef {
    let list = ListView::new()
        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 8.0, MARGIN, MARGIN))
        .children(rows);
    Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children([
            title_bar(library, resources, title),
            Expanded::new(list).into_widget(),
        ])
        .into_widget()
}

/// Back to the page beneath, and the page's `title`.
fn title_bar(library: &Entity<Library>, resources: &ThemeResources, title: &str) -> WidgetRef {
    let library = library.clone();
    let back = Listener::new(move |app| {
        library.update(app, |library, cx| library.go_back(cx));
    });
    let primary = resources.text_fill_color_primary;
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
            Text::new(title)
                .style(TextBlockStyle::Subtitle.text_style(resources, primary))
                .into_widget(),
        ]),
    )
    .into_widget()
}

/// A heading over a group of cards.
pub(crate) fn heading(text: &str, resources: &ThemeResources) -> WidgetRef {
    let primary = resources.text_fill_color_primary;
    Padding::new(EdgeInsetsGeometry::only(0.0, 16.0, 0.0, 8.0))
        .child(Text::new(text).style(TextBlockStyle::BodyStrong.text_style(resources, primary)))
        .into_widget()
}

/// The symbol of `permission`, as Windows's Settings marks each of its privacy pages.
fn permission_symbol(permission: Permission) -> FluentSymbol {
    match permission {
        Permission::Network => FluentSymbol::Globe,
        Permission::LocalNetwork => FluentSymbol::Desktop,
        Permission::Clipboard => FluentSymbol::Clipboard,
        Permission::Folders => FluentSymbol::Folder,
        Permission::Notifications => FluentSymbol::Alert,
        Permission::Camera => FluentSymbol::Camera,
        Permission::Microphone => FluentSymbol::Mic,
        Permission::ActivityRecord => FluentSymbol::Pulse,
    }
}

/// `symbol` as a card's mark, in the room an app's icon takes.
fn symbol(symbol: FluentSymbol, resources: &ThemeResources) -> WidgetRef {
    let glyph = FontIcon::symbol(symbol)
        .font_size(20.0)
        .foreground(resources.text_fill_color_primary);
    SizedBox::new()
        .width(MARK_SIDE)
        .height(MARK_SIDE)
        .child(Center::new().child(glyph))
        .into_widget()
}

/// A card that leads to `page`: its `mark` where it has one, `name` over what it holds where
/// that is said, and a chevron; a press anywhere on it opens the page.
pub(crate) fn link_card(
    library: &Entity<Library>,
    resources: &ThemeResources,
    mark: Option<WidgetRef>,
    name: &str,
    detail: Option<&str>,
    page: SettingsPage,
) -> WidgetRef {
    let mut lines = vec![
        Text::new(name)
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary))
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
    let text = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(lines);
    let said = Row::new().spacing(16.0).children(
        mark.into_iter()
            .chain([Expanded::new(text).into_widget(), chevron.into_widget()]),
    );
    let library = library.clone();
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
