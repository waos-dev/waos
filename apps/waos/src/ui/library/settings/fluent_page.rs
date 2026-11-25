//! What every desktop settings page is laid out with, so each reads on the same few edges, as
//! Microsoft's guidance for a settings page has it: Back in a row of its own over the page's
//! title, which starts where the cards do and names the page it came from before it; groups of
//! settings cards under headings; one control at a card's end, a drop-down as wide as every
//! other and a button no narrower than every other; and a list as a block, its rows starting
//! where its card's words do.
//!
//! The invariant: a card's words start after its icon, after the room of one, or at the card's
//! edge ([`Start`]), and one page uses the first two together or the last alone, so its words
//! share one edge.

use std::sync::Arc;

use waos_package::ProgramId;

use waft::{
    App, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxConstraints, BoxDecoration,
    Color, Column, ConstrainedBox, CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Entity,
    Expanded, FontWeight, IntoWidget, ListView, Listener, OverflowBox, Padding, Row, SizedBox,
    Text, TextOverflow, TextStyle, WidgetRef,
};
use waft_winui::{
    BreadcrumbBar, BreadcrumbBarItem, Button, ButtonStyle,
    CLICKABLE_SETTINGS_EXPANDER_ITEM_PADDING, ComboBox, ComboBoxItem, FluentSymbol, FontIcon,
    SETTINGS_CARD_HEADER_ICON_MAX_SIZE, SETTINGS_EXPANDER_ITEM_BORDER_THICKNESS,
    SETTINGS_EXPANDER_ITEM_MIN_HEIGHT, SettingsCard, SettingsExpander, TextBlockStyle,
    ThemeResources, ThemeScope, ToggleSwitch, right_aligned_compact_toggle_switch_style,
};

use super::{AppAction, AppInfo};
use crate::logic::permissions::Permission;
use crate::state::library::{AppDetail, Library, LibraryPage, SettingsPage};
use crate::ui::library::app_store::fluent_app_page::ICON_SIDE as APP_ICON_SIDE;
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

/// The room between two cards, as the Toolkit's settings pages stack them.
const CARD_SPACING: f64 = 4.0;

/// How wide every drop-down at a card's end is, so they share both edges.
const DROP_DOWN_WIDTH: f64 = 200.0;

/// The least width of a button at a card's end, so Add, Remove and their like share both edges.
const BUTTON_MIN_WIDTH: f64 = 96.0;

/// The page's title: WinUI's Title, 28 on a line of 36, semibold.
const TITLE_FONT_SIZE: f64 = 28.0;
const TITLE_LINE_HEIGHT: f64 = 36.0;

/// The chevron between two names of the title, in proportion to them.
const TITLE_CHEVRON_FONT_SIZE: f64 = 16.0;

/// The size a card's symbol is drawn at. The icon font's glyphs fill four fifths of their em
/// (Windows's own icon font fills it), so at this size their ink is as wide as the card's icon
/// room, where at the room's own size the card would show them a third smaller.
const SYMBOL_FONT_SIZE: f64 = 24.0;

/// The room over a heading, which parts its group from the one before, and under it.
const HEADING_MARGIN: [f64; 2] = [24.0, 6.0];

/// Where a card's words start.
pub(crate) enum Start {
    /// After the card's icon.
    Icon(WidgetRef),
    /// After the room of an icon, on a page whose other cards have one.
    Room,
    /// At the card's edge, on a page whose cards have no icons.
    Edge,
}

impl Start {
    /// After `symbol`.
    pub(crate) fn symbol(symbol: FluentSymbol) -> Start {
        Start::glyph(symbol.glyph())
    }

    /// After the icon font's `glyph`, drawn so its ink fills the card's icon room.
    pub(crate) fn glyph(glyph: char) -> Start {
        let drawn = FontIcon::new(glyph.to_string()).font_size(SYMBOL_FONT_SIZE);
        let room = SizedBox::new()
            .width(SETTINGS_CARD_HEADER_ICON_MAX_SIZE)
            .height(SETTINGS_CARD_HEADER_ICON_MAX_SIZE)
            .child(
                OverflowBox::new()
                    .max_width(f64::INFINITY)
                    .max_height(f64::INFINITY)
                    .child(drawn),
            );
        Start::Icon(room.into_widget())
    }

    /// After an app's icon.
    pub(crate) fn app_icon(png: Option<Arc<[u8]>>) -> Start {
        let icon = ProgramIcon {
            png,
            side: SETTINGS_CARD_HEADER_ICON_MAX_SIZE,
        };
        Start::Icon(icon.into_widget())
    }

    /// What the card shows as its icon.
    fn icon(self) -> Option<WidgetRef> {
        match self {
            Start::Icon(icon) => Some(icon),
            Start::Room => Some(SizedBox::shrink().into_widget()),
            Start::Edge => None,
        }
    }

    /// Where the rows under a card starting so start, from the block's edge: where the card's
    /// words do.
    fn row_padding(&self) -> [f64; 4] {
        let [after_icon, top, right, bottom] = CLICKABLE_SETTINGS_EXPANDER_ITEM_PADDING;
        match self {
            Start::Icon(_) | Start::Room => [after_icon, top, right, bottom],
            Start::Edge => [right, top, right, bottom],
        }
    }
}

/// A settings page: Back, the title naming `trail`, the pages down to this one, and `rows` under
/// it, which scroll.
pub(crate) fn page(
    library: &Entity<Library>,
    resources: &ThemeResources,
    trail: &[&str],
    rows: Vec<WidgetRef>,
) -> WidgetRef {
    let list = ListView::new()
        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, MARGIN))
        .children(rows);
    Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children([
            back(library),
            title(library, resources, trail),
            Expanded::new(list).into_widget(),
        ])
        .into_widget()
}

/// An app's own settings page: Back, and under it what scrolls, `header`, which says what the
/// app is, over `rows`.
pub(crate) fn app_page(
    library: &Entity<Library>,
    header: WidgetRef,
    rows: Vec<WidgetRef>,
) -> WidgetRef {
    let list = ListView::new()
        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 4.0, MARGIN, MARGIN))
        .children([header].into_iter().chain(rows));
    Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children([back(library), Expanded::new(list).into_widget()])
        .into_widget()
}

/// What the app `id` is, over its settings, as the app store's page says an app: its icon, and
/// beside it its name over its version and what it is doing now, then a button for each thing
/// that can be done with it.
pub(crate) fn app_header(
    library: &Entity<Library>,
    resources: &ThemeResources,
    id: &ProgramId,
    info: &AppInfo,
) -> WidgetRef {
    let icon = ProgramIcon {
        png: info.icon.clone(),
        side: APP_ICON_SIDE,
    };
    let (primary, secondary) = (
        resources.text_fill_color_primary,
        resources.text_fill_color_secondary,
    );
    let mut lines = vec![
        Text::new(info.name.clone())
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(TextBlockStyle::Subtitle.text_style(resources, primary))
            .into_widget(),
        Text::new(info.version.clone())
            .style(TextBlockStyle::Body.text_style(resources, secondary))
            .into_widget(),
    ];
    if let Some(status) = info.status {
        lines.push(
            Text::new(status)
                .style(TextBlockStyle::Body.text_style(resources, primary))
                .into_widget(),
        );
    }
    let heads_the_page = info.actions.iter().filter(|action| !action.ends_the_page());
    let buttons = heads_the_page.map(|action| {
        let (library, id, does) = (library.clone(), id.clone(), action.clone());
        let click = Listener::new(move |app| does.carry_out(&library, &id, app));
        let style = match action {
            AppAction::Update(_) => ButtonStyle::Accent,
            AppAction::Open
            | AppAction::Close
            | AppAction::ResetPermissions
            | AppAction::Remove => ButtonStyle::Default,
        };
        ConstrainedBox::new(BoxConstraints::new().min_width(BUTTON_MIN_WIDTH))
            .child(Button::text(action.label(), click).style(style))
            .into_widget()
    });
    lines.push(
        Padding::new(EdgeInsetsGeometry::only(0.0, 12.0, 0.0, 0.0))
            .child(Row::new().spacing(8.0).children(buttons))
            .into_widget(),
    );
    let said = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(lines);
    Row::new()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(16.0)
        .children([icon.into_widget(), Expanded::new(said).into_widget()])
        .into_widget()
}

/// What is done to the app `id` as a whole, rarely and not to be undone, at the end of its page
/// and apart from the rest: one card that unfolds, while `shown`, to a row for resetting its
/// permissions, its button dimmed while none is set otherwise, and a row for removing it.
/// `changed` hears the user fold or unfold it.
pub(crate) fn app_endings(
    library: &Entity<Library>,
    id: &ProgramId,
    info: &AppInfo,
    shown: bool,
    changed: impl Fn(&mut App, bool) + 'static,
) -> Vec<WidgetRef> {
    let can_reset = info
        .actions
        .iter()
        .any(|action| matches!(action, AppAction::ResetPermissions));
    let endings = [
        (
            AppAction::ResetPermissions,
            words::RESET_PERMISSIONS.to_owned(),
            words::RESET,
            can_reset,
        ),
        (
            AppAction::Remove,
            words::remove_app(&info.name),
            words::ASK_TO_REMOVE,
            true,
        ),
    ];
    let rows = endings.into_iter().map(|(action, name, button, enabled)| {
        let (library, id) = (library.clone(), id.clone());
        let click = Listener::new(move |app| action.carry_out(&library, &id, app));
        let button = ConstrainedBox::new(BoxConstraints::new().min_width(BUTTON_MIN_WIDTH))
            .child(Button::text(button, click).is_enabled(enabled));
        SettingsCard::new()
            .header(Text::new(name))
            .content(button)
            .into_widget()
    });
    let mut card = SettingsExpander::new(shown, changed)
        .header(Text::new(words::RESET_OR_REMOVE))
        .items(rows);
    card.header_icon = Start::symbol(FluentSymbol::ArrowRepeatAll).icon();
    let apart = SizedBox::new().height(HEADING_MARGIN[0]).into_widget();
    vec![apart, spaced(card)]
}

/// Back to the page beneath, in a row of its own, its arrow over the cards' edge.
fn back(library: &Entity<Library>) -> WidgetRef {
    let library = library.clone();
    let back = Listener::new(move |app| {
        library.update(app, |library, cx| library.go_back(cx));
    });
    let button = Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
        .style(ButtonStyle::Subtle)
        .padding([8.0, 8.0, 8.0, 8.0]);
    Padding::new(EdgeInsetsGeometry::from_ltrb(
        MARGIN - 8.0,
        8.0,
        MARGIN,
        0.0,
    ))
    .child(Row::new().children([button.into_widget()]))
    .into_widget()
}

/// The page's title, starting where the cards do: the names of `trail`, the last the page's
/// own, each before it leading back to its page, as Windows's Settings titles a page.
fn title(library: &Entity<Library>, resources: &ThemeResources, trail: &[&str]) -> WidgetRef {
    let named = TextStyle::new()
        .font_size(TITLE_FONT_SIZE)
        .font_weight(FontWeight::W600)
        .height(TITLE_LINE_HEIGHT / TITLE_FONT_SIZE);
    let items = trail
        .iter()
        .enumerate()
        .map(|(index, name)| {
            BreadcrumbBarItem::new(index.to_string(), Text::new(*name).style(named.clone()))
        })
        .collect();
    let (library, pages) = (library.clone(), trail.len());
    let names = BreadcrumbBar::new(items, move |app, clicked| {
        let back_by = pages.saturating_sub(clicked.index + 1);
        library.update(app, |library, cx| {
            for _ in 0..back_by {
                library.go_back(cx);
            }
        });
    });
    // WinUI's way to restyle one control: the resources its template reads. A page before this
    // one is said more quietly than this one, as Windows's Settings says it.
    let (secondary, primary) = (
        resources.text_fill_color_secondary,
        resources.text_fill_color_primary,
    );
    let names = ThemeScope::overriding(names).any(move |resources| {
        resources.breadcrumb_bar_chevron_font_size = TITLE_CHEVRON_FONT_SIZE;
        resources.breadcrumb_bar_normal_foreground_brush = secondary;
        resources.breadcrumb_bar_hover_foreground_brush = primary;
    });
    Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 16.0))
        .child(Row::new().children([names.into_widget()]))
        .into_widget()
}

/// A heading over a group of cards.
pub(crate) fn heading(text: &str, resources: &ThemeResources) -> WidgetRef {
    let primary = resources.text_fill_color_primary;
    let [over, under] = HEADING_MARGIN;
    Padding::new(EdgeInsetsGeometry::only(1.0, over, 0.0, under))
        .child(Text::new(text).style(TextBlockStyle::BodyStrong.text_style(resources, primary)))
        .into_widget()
}

/// What a page says over its cards: caption-sized, in the secondary colour, with the room under
/// it that sets it apart from them.
pub(crate) fn note(resources: &ThemeResources, text: &str) -> WidgetRef {
    let secondary = resources.text_fill_color_secondary;
    Padding::new(EdgeInsetsGeometry::only(1.0, 0.0, 0.0, 8.0))
        .child(Text::new(text).style(TextBlockStyle::Caption.text_style(resources, secondary)))
        .into_widget()
}

/// `card` with the room under it that parts it from the next.
pub(crate) fn spaced<K>(card: impl IntoWidget<K>) -> WidgetRef {
    Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, CARD_SPACING))
        .child(card.into_widget())
        .into_widget()
}

/// A settings card's parts: its words starting at `start`, `name` over `detail` where that is
/// said, and `control` at its end where it has one.
fn card(
    start: Start,
    name: &str,
    detail: Option<&str>,
    control: Option<WidgetRef>,
) -> SettingsCard {
    let mut card = SettingsCard::new().header(Text::new(name));
    card.header_icon = start.icon();
    card.description = detail.map(|detail| Text::new(detail).into_widget());
    card.content = control;
    card
}

/// One setting, a settings card: `name` over `detail` where that is said, and its one `control`
/// at its end.
pub(crate) fn setting_card(
    start: Start,
    name: &str,
    detail: Option<&str>,
    control: Option<WidgetRef>,
) -> WidgetRef {
    spaced(card(start, name, detail, control))
}

/// A settings card to click that leads to `page`: `name` over what it holds where that is said,
/// and the chevron a card to click ends with.
pub(crate) fn link_card(
    library: &Entity<Library>,
    start: Start,
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
        card(start, name, detail, None)
            .is_click_enabled(true)
            .click(open),
    )
}

/// One of a list: what it is, and the one control at its end where it has one.
pub(crate) struct ListRow {
    pub(crate) text: String,
    pub(crate) control: Option<WidgetRef>,
}

/// A list as one block: a card saying what it lists (`name` over `detail`, `control` at its end,
/// as [`setting_card`]), and under it its `rows`, each starting where the card's words do, a
/// line between two.
pub(crate) fn list_block(
    resources: &ThemeResources,
    start: Start,
    name: &str,
    detail: Option<&str>,
    control: Option<WidgetRef>,
    rows: Vec<ListRow>,
) -> WidgetRef {
    let row_padding = start.row_padding();
    // The block draws the card's ground and edge once, around the card and its rows.
    let head = card(start, name, detail, control)
        .background(Color::from_argb(0, 0, 0, 0))
        .border_thickness([0.0; 4]);
    let rows = rows.into_iter().map(|row| {
        let mut listed = SettingsCard::new()
            .header(Text::new(row.text))
            .background(Color::from_argb(0, 0, 0, 0))
            .border_thickness(SETTINGS_EXPANDER_ITEM_BORDER_THICKNESS)
            .corner_radius(0.0)
            .min_height(SETTINGS_EXPANDER_ITEM_MIN_HEIGHT)
            .padding(row_padding);
        listed.content = row.control;
        listed.into_widget()
    });
    let surface = BoxDecoration {
        color: Some(resources.card_background_fill_color_default.into()),
        border: Some(
            Border::all(
                resources.card_stroke_color_default,
                1.0,
                BorderStyle::Solid,
                BorderSide::STROKE_ALIGN_INSIDE,
            )
            .into(),
        ),
        border_radius: Some(BorderRadiusGeometry::circular(
            resources.control_corner_radius[0],
        )),
        ..BoxDecoration::default()
    };
    let block = Column::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children([head.into_widget()].into_iter().chain(rows));
    spaced(
        DecoratedBox::new(surface).child(Padding::new(EdgeInsetsGeometry::all(1.0)).child(block)),
    )
}

/// A switch that is `on` or off, in the style a settings card gives the switch at its end,
/// telling `changed` when the user flips it.
pub(crate) fn card_switch(on: bool, changed: impl Fn(&mut App, bool) + 'static) -> WidgetRef {
    ToggleSwitch::new(on, changed)
        .template(right_aligned_compact_toggle_switch_style)
        .into_widget()
}

/// A button at a card's end, saying `label` and doing `click`, no narrower than every other.
pub(crate) fn card_button(label: &str, click: Listener) -> WidgetRef {
    ConstrainedBox::new(BoxConstraints::new().min_width(BUTTON_MIN_WIDTH))
        .child(Button::text(label, click))
        .into_widget()
}

/// A drop-down at a card's end, as wide as every other, of `choices`: each its name, whether it
/// is the one chosen, and what choosing it does.
pub(crate) fn drop_down(choices: Vec<(String, bool, Listener)>) -> WidgetRef {
    let chosen = choices.iter().position(|(_, chosen, _)| *chosen);
    let items = choices
        .iter()
        .enumerate()
        .map(|(index, (name, ..))| ComboBoxItem::text(index.to_string(), name.clone()))
        .collect();
    let chooses: Vec<Listener> = choices.into_iter().map(|(.., choose)| choose).collect();
    let pick = ComboBox::new(items, chosen, move |app, picked| {
        let picked = picked.selected_index();
        if picked != chosen
            && let Some(choose) = picked.and_then(|index| chooses.get(index))
        {
            choose.call(app);
        }
    });
    SizedBox::new()
        .width(DROP_DOWN_WIDTH)
        .child(pick)
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

/// The glyph of what is an app's alone, in the icon font: a server for its machines, a document
/// for its file pickers, a speaker for its sound output, a link for its links, a ribbon for its
/// roles.
pub(crate) fn detail_glyph(detail: AppDetail) -> char {
    match detail {
        AppDetail::Machines => '\u{f769}',
        AppDetail::FilePickers => '\u{f378}',
        AppDetail::SoundOutput => '\u{eb42}',
        AppDetail::Links => '\u{f4e4}',
        AppDetail::Roles => '\u{ea27}',
    }
}

/// The words a setting that is on or off says for each.
pub(crate) fn on_and_off(permission: Permission) -> (&'static str, &'static str) {
    match permission {
        Permission::ActivityRecord => (words::ON, words::OFF),
        _ => (words::ALLOW, words::DONT_ALLOW),
    }
}
