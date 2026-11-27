//! The home at a phone's size, in Cupertino's widgets, in the proportions of iOS's home screen:
//! a navigation bar saying WaOS with a plus, which gets programs, and a gear, which opens every
//! program's settings, at its trailing end; under it four columns of 60-point icons, each name on
//! one line in the caption size, the apps used often in a row of their own over a hairline where
//! the apps fill more than a row, each icon sliding to its place as the icons change
//! (`icon_grid`). A tap opens the program; holding it offers its actions
//! (`icon_actions::cupertino`). While the user searches, only the programs searched for.
//!
//! What it shows is the library's, read as it builds; it keeps nothing of its own.

use waft::{
    App, BuildContext, Center, Column, EdgeInsetsGeometry, Entity, Expanded, Icon, IntoWidget,
    Listener, MainAxisSize, Padding, Row, Size, SizedBox, StatelessWidget, Text, TextAlign,
    TextStyle, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoColors, CupertinoDynamicColor, CupertinoIcons,
    CupertinoNavigationBar, CupertinoPageScaffold, ObstructingPreferredSizeWidgetRef,
};

use super::icon_grid::{GridLook, IconGrid, SectionLineKind};
use super::{HomeEntry, IconTile, TileLook, home_entries, often_used_entries};
use crate::state::library::{Library, LibraryPage, SettingsPage};
use crate::state::recordings::Recordings;
use crate::ui::library::cupertino::under_bar;
use crate::ui::library::icon_actions::cupertino::show_icon_actions;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::recording::cupertino_recording;
use crate::ui::library::words;
use waos_package::ProgramId;

/// iOS's home screen on a phone 390 points wide: four columns of 60-point icons, 27 points in
/// from each side, a name 6 points under its icon in 12-point type, rows 28 points apart.
const COLUMNS: usize = 4;
/// The size of the symbol over a home with nothing installed: iOS's empty views'.
const EMPTY_SYMBOL_SIZE: f64 = 56.0;
const ICON_SIDE: f64 = 60.0;
const SIDE_MARGIN: f64 = 27.0;
const TOP_MARGIN: f64 = 20.0;
const NAME_GAP: f64 = 6.0;
const NAME_SIZE: f64 = 12.0;
const ROW_GAP: f64 = 28.0;

/// The home, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoHome {
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
}

impl StatelessWidget for CupertinoHome {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let entries = home_entries(self.library.read(app));
        let searching = !self.library.read(app).search().trim().is_empty();
        let banner = cupertino_recording(&self.recordings, app, context);
        let body = if entries.is_empty() {
            let nothing_installed = if searching {
                nothing_found()
            } else {
                self.nothing_installed(app, context)
            };
            under_bar(move |top| {
                Column::new()
                    .children(
                        banner
                            .clone()
                            .map(|banner| padded_banner(banner, top))
                            .into_iter()
                            .chain([Expanded::new(nothing_installed.clone()).into_widget()]),
                    )
                    .into_widget()
            })
        } else {
            let over = banner.map(|banner| {
                Padding::new(EdgeInsetsGeometry::from_ltrb(
                    SIDE_MARGIN,
                    TOP_MARGIN,
                    SIDE_MARGIN,
                    0.0,
                ))
                .child(banner)
                .into_widget()
            });
            let often_used = often_used_entries(self.library.read(app));
            let look = self.tile_look(app, context);
            let line_color = resolved(CupertinoColors::SEPARATOR, app, context);
            let tiles_of = |entries: Vec<HomeEntry>| -> Vec<(ProgramId, WidgetRef)> {
                entries
                    .into_iter()
                    .map(|entry| {
                        let id = entry.id.clone();
                        (id, self.tile(entry, &look, context))
                    })
                    .collect()
            };
            let (apps, often_used) = (tiles_of(entries), tiles_of(often_used));
            under_bar(move |top| {
                IconGrid {
                    apps: apps.clone(),
                    often_used: often_used.clone(),
                    over: over.clone(),
                    highlighted: None,
                    look: GridLook {
                        columns: |_| COLUMNS,
                        side: SIDE_MARGIN - CELL_PADDING,
                        line_side: SIDE_MARGIN,
                        under_a_bar: top,
                        section_top: TOP_MARGIN,
                        row_gap: ROW_GAP,
                        bottom: TOP_MARGIN,
                        line: SectionLineKind::BetweenSections,
                        line_color,
                        highlight: None,
                    },
                }
                .into_widget()
            })
        };
        let bar = CupertinoNavigationBar::new()
            .middle(Text::new(words::HOME_TITLE))
            .trailing(self.bar_buttons());
        CupertinoPageScaffold::new(body)
            .navigation_bar(ObstructingPreferredSizeWidgetRef::new(bar))
            .background_color(CupertinoColors::SYSTEM_BACKGROUND)
            .into_widget()
    }
}

/// How far a tile's tap area reaches past its icon on each side.
const CELL_PADDING: f64 = 4.0;

impl CupertinoHome {
    /// The plus, which gets programs, and the gear, which opens every program's settings.
    fn bar_buttons(&self) -> WidgetRef {
        Row::new()
            .main_axis_size(MainAxisSize::Min)
            .children([
                bar_button(CupertinoIcons::add(), self.show(LibraryPage::AppStore)),
                bar_button(
                    CupertinoIcons::gear_alt(),
                    self.show(LibraryPage::Settings(SettingsPage::Root)),
                ),
            ])
            .into_widget()
    }

    /// How an icon's name and running dot look.
    fn tile_look(&self, app: &mut App, context: BuildContext) -> TileLook {
        TileLook {
            name_style: TextStyle::new()
                .color(resolved(CupertinoColors::LABEL, app, context))
                .font_size(NAME_SIZE),
            name_gap: NAME_GAP,
            dot_color: resolved(CupertinoColors::SECONDARY_LABEL, app, context),
        }
    }

    /// The program's icon and name, which opens it at a tap.
    fn tile(&self, entry: HomeEntry, look: &TileLook, context: BuildContext) -> WidgetRef {
        let icon = ProgramIcon {
            png: entry.icon.clone(),
            side: ICON_SIDE,
        };
        let tile = IconTile {
            icon: icon.into_widget(),
            name: entry.name.clone(),
            running: entry.running,
            status: entry.status,
            look: look.clone(),
        };
        let (library, id) = (self.library.clone(), entry.id.clone());
        let open =
            Listener::new(move |app| library.update(app, |library, cx| library.open(&id, cx)));
        let (library, id) = (self.library.clone(), entry.id);
        let hold = Listener::new(move |app| show_icon_actions(app, context, &library, &id));
        CupertinoButton::new(tile.into_widget(), Some(open))
            .on_long_press(hold)
            .padding(EdgeInsetsGeometry::all(CELL_PADDING))
            .minimum_size(Size::new(0.0, 0.0))
            .into_widget()
    }

    /// Nothing installed, as iOS's empty views say it (`ContentUnavailableView`): a large
    /// symbol, a title, what will appear here, and the button that gets programs.
    fn nothing_installed(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let secondary = resolved(CupertinoColors::SECONDARY_LABEL, app, context);
        let label = resolved(CupertinoColors::LABEL, app, context);
        let symbol = Icon::new(Some(CupertinoIcons::square_grid_2x2()))
            .size(EMPTY_SYMBOL_SIZE)
            .color(secondary);
        let title = Text::new(words::NO_PROGRAMS)
            .text_align(TextAlign::Center)
            .style(
                TextStyle::new()
                    .font_size(20.0)
                    .font_weight(waft::ui::FontWeight::W600)
                    .color(label),
            );
        let said = Text::new(words::NO_PROGRAMS_SAID)
            .text_align(TextAlign::Center)
            .style(TextStyle::new().font_size(15.0).color(secondary));
        let get = CupertinoButton::tinted(
            Text::new(words::GET_PROGRAMS).into_widget(),
            Some(self.show(LibraryPage::AppStore)),
        );
        Center::new()
            .child(Padding::new(EdgeInsetsGeometry::all(SIDE_MARGIN)).child(
                Column::new().main_axis_size(MainAxisSize::Min).children([
                    symbol.into_widget(),
                    SizedBox::new().height(16.0).into_widget(),
                    title.into_widget(),
                    SizedBox::new().height(6.0).into_widget(),
                    said.into_widget(),
                    SizedBox::new().height(20.0).into_widget(),
                    get.into_widget(),
                ]),
            ))
            .into_widget()
    }

    /// A tap that takes the library to `page`.
    fn show(&self, page: LibraryPage) -> Listener {
        let library = self.library.clone();
        Listener::new(move |app| {
            let page = page.clone();
            library.update(app, |library, cx| library.show_page(page, cx));
        })
    }
}

/// One line saying no installed program is what the user searches for.
fn nothing_found() -> WidgetRef {
    Center::new()
        .child(
            Padding::new(EdgeInsetsGeometry::all(SIDE_MARGIN))
                .child(Text::new(words::NO_PROGRAMS_FOUND).text_align(TextAlign::Center)),
        )
        .into_widget()
}

/// The banner of who records, above the home's message, under the bar.
fn padded_banner(banner: WidgetRef, top: f64) -> WidgetRef {
    Padding::new(EdgeInsetsGeometry::from_ltrb(
        SIDE_MARGIN,
        top + TOP_MARGIN,
        SIDE_MARGIN,
        0.0,
    ))
    .child(banner)
    .into_widget()
}

/// An icon button of the navigation bar.
fn bar_button(icon: waft::IconData, tap: Listener) -> WidgetRef {
    CupertinoButton::new(Icon::new(Some(icon)).into_widget(), Some(tap))
        .padding(EdgeInsetsGeometry::symmetric(0.0, 8.0))
        .into_widget()
}

/// `color` as the theme of `context` resolves it.
pub(crate) fn resolved(color: waft::AnyColor, app: &mut App, context: BuildContext) -> waft::Color {
    CupertinoDynamicColor::resolve(&color, app, context).color()
}
