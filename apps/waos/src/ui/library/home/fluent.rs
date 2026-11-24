//! The home at a desktop's or a tablet's width, in Fluent's controls, as the Start menu's pinned
//! apps are: a title bar saying WaOS with a plus, which gets programs, and a gear, which opens
//! every program's settings, at its trailing end, over the home's body (`FluentHomeBody`), which
//! the desktop's launcher puts under its search row instead (`ui::launcher`): who records, then the
//! icons, 64 points each as Finder shows them by default, in as many columns as fit, up to seven,
//! each name on one line in the caption size, each icon a subtle button. A click opens the program;
//! a right click offers its actions (`icon_actions::fluent`). With nothing installed, one line and
//! a button that gets programs; with nothing found, one line.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BuildContext, Center, Column, CrossAxisAlignment,
    DecoratedBox, EdgeInsetsGeometry, Entity, Expanded, IntoWidget, Listener, MainAxisSize,
    Padding, Row, SizedBox, StatelessWidget, Text, TextAlign, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, InfoBadgeStyle, TextBlockStyle, ThemeResources,
    ThemeScope,
};

use super::icon_grid::{GridLook, Highlighted, IconGrid, SectionLineKind};
use super::{HomeEntry, IconTile, TileLook, badge_dot, home_entries, often_used_entries};
use crate::logic::setting_changes::SettingsOf;
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;
use crate::ui::library::category_chips::category_chips;
use crate::ui::library::icon_actions::fluent::IconMenu;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::recording::fluent_recording;
use crate::ui::library::words;
use waos_package::ProgramId;

/// The home's margins, as the Start menu's.
pub(crate) const MARGIN: f64 = 24.0;
/// The room above the grid's first row, which scrolls away with it.
const GRID_TOP: f64 = 12.0;
/// How round the highlight behind the icon the arrow keys are on is: WinUI's control corners.
const HIGHLIGHT_CORNER_RADIUS: f64 = 4.0;
/// The size of the symbol over a home with nothing installed.
const EMPTY_SYMBOL_SIZE: f64 = 48.0;
const TITLE_TOP: f64 = 16.0;

/// The icons: 64 points, in cells at least 96 wide, up to seven to a row.
const ICON_SIDE: f64 = 64.0;
const MIN_CELL: f64 = 96.0;
const MAX_COLUMNS: usize = 7;
const NAME_GAP: f64 = 6.0;
const ROW_GAP: f64 = 4.0;
const TILE_PADDING: [f64; 4] = [4.0, 8.0, 4.0, 6.0];

/// The badge on an icon whose program a source lists newer.
const BADGE_SIDE: f64 = 12.0;

/// The size of the title bar's icons.
const BAR_ICON_SIZE: f64 = 16.0;

/// The home, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentHome {
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
}

impl StatelessWidget for FluentHome {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let body = FluentHomeBody {
            library: self.library.clone(),
            recordings: self.recordings.clone(),
            highlighted: None,
        };
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                self.title_bar(&resources),
                Expanded::new(body).into_widget(),
            ])
            .into_widget()
    }
}

impl FluentHome {
    /// WaOS, and the plus and the gear at the trailing end.
    fn title_bar(&self, resources: &ThemeResources) -> WidgetRef {
        let title = Text::new(words::HOME_TITLE).style(
            TextBlockStyle::Subtitle.text_style(resources, resources.text_fill_color_primary),
        );
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN,
            TITLE_TOP,
            MARGIN - 8.0,
            12.0,
        ))
        .child(Row::new().spacing(4.0).children([
            Expanded::new(title).into_widget(),
            bar_button(
                FluentSymbol::Add,
                show(&self.library, LibraryPage::AppStore),
            ),
            bar_button(
                FluentSymbol::Settings,
                show(&self.library, LibraryPage::Settings(SettingsOf::Global)),
            ),
        ]))
        .into_widget()
    }
}

/// The home under its top row: who records, then the installed programs' icons, or one line where
/// there are none to show.
#[derive(Debug)]
pub(crate) struct FluentHomeBody {
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
    /// The icon the arrow keys are on, where whoever shows the body moves a highlight by them.
    pub(crate) highlighted: Option<Highlighted>,
}

/// What shows that the arrow keys are on an icon: Fluent's subtle fill behind it, as the item
/// the keyboard is on shows in a WinUI list.
#[derive(Debug)]
pub(crate) struct IconHighlight {
    child: WidgetRef,
}

impl StatelessWidget for IconHighlight {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let fill = BoxDecoration {
            color: Some(resources.subtle_fill_color_secondary.into()),
            border_radius: Some(BorderRadiusGeometry::circular(HIGHLIGHT_CORNER_RADIUS)),
            ..BoxDecoration::default()
        };
        DecoratedBox::new(fill)
            .child(self.child.clone())
            .into_widget()
    }
}

/// How many columns of icons a home `width` wide holds.
pub(crate) fn columns_in(width: f64) -> usize {
    columns_for(width - 2.0 * (MARGIN - TILE_PADDING[0]))
}

impl StatelessWidget for FluentHomeBody {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let mut children = Vec::new();
        if let Some(banner) = fluent_recording(&self.recordings, app, context) {
            children.push(
                Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 8.0))
                    .child(banner)
                    .into_widget(),
            );
        }
        let library = self.library.read(app);
        // A category chosen is a token in the search field, which says what shows; the chips
        // are what chooses one.
        let categories = library.categories();
        if !categories.is_empty() && library.category().is_none() {
            let chosen = self.library.clone();
            children.push(category_chips(&categories, MARGIN, move |app, category| {
                chosen.update(app, |library, cx| {
                    library.show_category(Some(category), cx);
                });
            }));
        }
        let entries = home_entries(library);
        let content = if !entries.is_empty() {
            self.grid(entries, often_used_entries(library), &resources)
        } else if library.search().trim().is_empty() {
            self.nothing_installed(&resources)
        } else {
            nothing_found(&resources)
        };
        children.push(Expanded::new(content).into_widget());
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children)
            .into_widget()
    }
}

impl FluentHomeBody {
    /// The installed apps' icons and, over them, those `often_used`, as an [`IconGrid`] shows
    /// them.
    fn grid(
        &self,
        entries: Vec<HomeEntry>,
        often_used: Vec<HomeEntry>,
        resources: &ThemeResources,
    ) -> WidgetRef {
        let look = TileLook {
            name_style: TextBlockStyle::Caption
                .text_style(resources, resources.text_fill_color_primary),
            name_gap: NAME_GAP,
            dot_color: resources.text_fill_color_secondary,
        };
        let badge_color = InfoBadgeStyle::Attention.background(resources);
        let tiles_of = |entries: Vec<HomeEntry>| -> Vec<(ProgramId, WidgetRef)> {
            entries
                .into_iter()
                .map(|entry| (entry.id.clone(), self.tile(entry, &look, badge_color)))
                .collect()
        };
        let grid = IconGrid {
            apps: tiles_of(entries),
            often_used: tiles_of(often_used),
            over: None,
            highlighted: self.highlighted.clone(),
            look: GridLook {
                columns: columns_in,
                side: MARGIN - TILE_PADDING[0],
                line_side: MARGIN,
                under_a_bar: 0.0,
                section_top: GRID_TOP,
                row_gap: ROW_GAP,
                bottom: MARGIN,
                line: SectionLineKind::Stays,
                line_color: resources.divider_stroke_color_default,
                highlight: Some(|icon| IconHighlight { child: icon }.into_widget()),
            },
        };
        // An icon answers a press, as a button does, and shows nothing for the pointer resting
        // over it: WinUI's way to restyle a subtree's buttons.
        ThemeScope::overriding(grid)
            .any(|resources| {
                resources.subtle_button_background_pointer_over =
                    resources.subtle_button_background;
            })
            .into_widget()
    }

    /// The program's icon and name, a subtle button that opens it at a click.
    fn tile(&self, entry: HomeEntry, look: &TileLook, badge_color: waft::Color) -> WidgetRef {
        let badge = entry
            .update_offered
            .then(|| (badge_dot(badge_color, BADGE_SIDE), BADGE_SIDE));
        let icon = ProgramIcon {
            png: entry.icon.clone(),
            side: ICON_SIDE,
            badge,
        };
        let tile = IconTile {
            icon: icon.into_widget(),
            name: entry.name.clone(),
            running: entry.running,
            installing: entry.installing,
            look: look.clone(),
        };
        let (library, id) = (self.library.clone(), entry.id.clone());
        let open =
            Listener::new(move |app| library.update(app, |library, cx| library.open(&id, cx)));
        let button = Button::new(tile, open)
            .style(ButtonStyle::Subtle)
            .padding(TILE_PADDING);
        IconMenu {
            library: self.library.clone(),
            id: entry.id,
            child: button.into_widget(),
        }
        .into_widget()
    }

    /// Nothing installed, as an empty view says it (Apple's `ContentUnavailableView`, in
    /// Fluent's symbols and type): the apps symbol, a title, what will appear here, and the
    /// button that gets programs.
    fn nothing_installed(&self, resources: &ThemeResources) -> WidgetRef {
        let symbol = FontIcon::symbol(FluentSymbol::Apps)
            .font_size(EMPTY_SYMBOL_SIZE)
            .foreground(resources.text_fill_color_secondary);
        let title = Text::new(words::NO_PROGRAMS)
            .text_align(TextAlign::Center)
            .style(
                TextBlockStyle::Subtitle.text_style(resources, resources.text_fill_color_primary),
            );
        let said = Text::new(words::NO_PROGRAMS_SAID)
            .text_align(TextAlign::Center)
            .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary));
        let get = Button::text(
            words::GET_PROGRAMS,
            show(&self.library, LibraryPage::AppStore),
        )
        .style(ButtonStyle::Accent);
        Center::new()
            .child(
                Padding::new(EdgeInsetsGeometry::from_ltrb(
                    MARGIN,
                    0.0,
                    MARGIN,
                    2.0 * MARGIN,
                ))
                .child(Column::new().main_axis_size(MainAxisSize::Min).children(
                    [
                        symbol.into_widget(),
                        SizedBox::new().height(16.0).into_widget(),
                        title.into_widget(),
                        SizedBox::new().height(4.0).into_widget(),
                        said.into_widget(),
                        SizedBox::new().height(20.0).into_widget(),
                        get.into_widget(),
                    ],
                )),
            )
            .into_widget()
    }
}

/// One line saying no program installed is the one searched for.
fn nothing_found(resources: &ThemeResources) -> WidgetRef {
    let said = Text::new(words::NO_PROGRAMS_FOUND)
        .text_align(TextAlign::Center)
        .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary));
    Center::new().child(said).into_widget()
}

/// A click that takes `library` to `page`.
pub(crate) fn show(library: &Entity<Library>, page: LibraryPage) -> Listener {
    let library = library.clone();
    Listener::new(move |app| {
        let page = page.clone();
        library.update(app, |library, cx| library.show_page(page, cx));
    })
}

/// How many columns of icons fit in `width`: as many cells as fit, up to seven.
fn columns_for(width: f64) -> usize {
    let fit = (width / MIN_CELL).floor();
    if fit.is_finite() && fit >= 1.0 {
        (fit as usize).min(MAX_COLUMNS)
    } else {
        1
    }
}

/// An icon button of the title bar.
fn bar_button(symbol: FluentSymbol, click: Listener) -> WidgetRef {
    Button::new(FontIcon::symbol(symbol).font_size(BAR_ICON_SIZE), click)
        .style(ButtonStyle::Subtle)
        .padding([8.0, 8.0, 8.0, 8.0])
        .into_widget()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_many_columns_as_fit_up_to_seven() {
        assert_eq!(columns_for(50.0), 1);
        assert_eq!(columns_for(300.0), 3);
        assert_eq!(columns_for(680.0), 7);
        assert_eq!(columns_for(2000.0), 7);
    }
}
