//! The settings' pages that lead to others, at a desktop's width, in Fluent's controls, as
//! Windows's Settings leads to its pages: a settings card to click for each place, its icon, its
//! name and what it holds. The first page leads to General, where this system has settings of
//! WaOS's own, to Permissions, and, under Apps, to each installed app; Permissions leads to
//! each kind, saying what its default is. And what every desktop settings page is laid out
//! with: its title bar, its headings and notes, and the room between its cards.
//!
//! What they show is the library's, read as they build; they keep nothing of their own.

use std::rc::Rc;
use std::sync::Arc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded,
    IntoWidget, ListView, Listener, Padding, Row, StatelessWidget, Text, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, SETTINGS_CARD_HEADER_ICON_MAX_SIZE, SettingsCard,
    TextBlockStyle, ThemeResources, ToggleSwitch, right_aligned_compact_toggle_switch_style,
};

use super::{listed_apps, permission_defaults, waos_has_settings};
use crate::logic::permissions::Permission;
use crate::state::library::{Library, LibraryPage, SettingsPage};
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The room between two cards, as the Toolkit's settings pages stack them.
const CARD_SPACING: f64 = 4.0;

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
                Some(FontIcon::symbol(FluentSymbol::Settings).into_widget()),
                words::WAOS_SETTINGS,
                Some(words::WAOS_SETTINGS_DETAIL),
                SettingsPage::General,
            ));
        }
        rows.push(link_card(
            &self.library,
            Some(FontIcon::symbol(FluentSymbol::Shield).into_widget()),
            words::PERMISSIONS,
            Some(words::PERMISSIONS_DETAIL),
            SettingsPage::Permissions,
        ));
        let apps = listed_apps(library);
        if !apps.is_empty() {
            rows.push(heading(words::APPS, &resources));
        }
        for listed in apps {
            rows.push(link_card(
                &self.library,
                Some(app_icon(listed.icon)),
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
                    Some(FontIcon::symbol(permission_symbol(permission)).into_widget()),
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
pub(crate) fn title_bar(
    library: &Entity<Library>,
    resources: &ThemeResources,
    title: &str,
) -> WidgetRef {
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
pub(crate) fn permission_symbol(permission: Permission) -> FluentSymbol {
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

/// An app's icon, at the side a settings card shows its icon.
pub(crate) fn app_icon(png: Option<Arc<[u8]>>) -> WidgetRef {
    ProgramIcon {
        png,
        side: SETTINGS_CARD_HEADER_ICON_MAX_SIZE,
    }
    .into_widget()
}

/// What a page says where it sets nothing: caption-sized, in the secondary colour.
pub(crate) fn note(resources: &ThemeResources, text: &str) -> WidgetRef {
    Text::new(text)
        .style(TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary))
        .into_widget()
}

/// `card` with the room under it that parts it from the next.
pub(crate) fn spaced<K>(card: impl IntoWidget<K>) -> WidgetRef {
    Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, CARD_SPACING))
        .child(card.into_widget())
        .into_widget()
}

/// A settings card: `icon` where it has one, `name` over `detail` where that is said, and
/// `control` at its end where it has one.
pub(crate) fn setting_card(
    icon: Option<WidgetRef>,
    name: &str,
    detail: Option<&str>,
    control: Option<WidgetRef>,
) -> SettingsCard {
    let mut card = SettingsCard::new().header(Text::new(name));
    card.header_icon = icon;
    card.description = detail.map(|detail| Text::new(detail).into_widget());
    card.content = control;
    card
}

/// A settings card to click that leads to `page`: its `icon` where it has one, `name` over what
/// it holds where that is said, and the chevron a card to click ends with.
pub(crate) fn link_card(
    library: &Entity<Library>,
    icon: Option<WidgetRef>,
    name: &str,
    detail: Option<&str>,
    page: SettingsPage,
) -> WidgetRef {
    let library = library.clone();
    let open = Listener::new(move |app| {
        let page = LibraryPage::Settings(page.clone());
        library.update(app, |library, cx| library.show_page(page, cx));
    });
    spaced(
        setting_card(icon, name, detail, None)
            .is_click_enabled(true)
            .click(open),
    )
}

/// A switch that is `on` or off, in the style a settings card gives the switch at its end,
/// telling `changed` when the user flips it.
pub(crate) fn card_switch(on: bool, changed: impl Fn(&mut App, bool) + 'static) -> WidgetRef {
    ToggleSwitch::new(on, changed)
        .template(right_aligned_compact_toggle_switch_style)
        .into_widget()
}
