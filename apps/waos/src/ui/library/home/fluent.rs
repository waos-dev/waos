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
    App, BoxConstraints, BuildContext, Center, Column, CrossAxisAlignment, EdgeInsetsGeometry,
    Entity, Expanded, IntoWidget, LayoutBuilder, ListView, Listener, MainAxisSize, Padding, Row,
    StatelessWidget, Text, TextAlign, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, InfoBadgeStyle, TextBlockStyle, ThemeResources,
};

use super::{HomeEntry, IconTile, TileLook, badge_dot, grid_rows, home_entries};
use crate::logic::setting_changes::SettingsOf;
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;
use crate::ui::library::icon_actions::fluent::IconMenu;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::recording::fluent_recording;
use crate::ui::library::words;

/// The home's margins, as the Start menu's.
pub(crate) const MARGIN: f64 = 24.0;
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
                show(&self.library, LibraryPage::GetPrograms),
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
        let entries = home_entries(library);
        let content = if !entries.is_empty() {
            self.grid(entries, &resources)
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
    /// The installed programs' icons, in as many columns as the panel's width holds.
    fn grid(&self, entries: Vec<HomeEntry>, resources: &ThemeResources) -> WidgetRef {
        let look = TileLook {
            name_style: TextBlockStyle::Caption
                .text_style(resources, resources.text_fill_color_primary),
            name_gap: NAME_GAP,
            dot_color: resources.text_fill_color_secondary,
        };
        let badge_color = InfoBadgeStyle::Attention.background(resources);
        let tiles: Vec<WidgetRef> = entries
            .into_iter()
            .map(|entry| self.tile(entry, &look, badge_color))
            .collect();
        LayoutBuilder::new(move |_app, _context, constraints: BoxConstraints| {
            let columns = columns_for(constraints.max_width - 2.0 * (MARGIN - TILE_PADDING[0]));
            ListView::new()
                .padding(EdgeInsetsGeometry::from_ltrb(
                    MARGIN - TILE_PADDING[0],
                    0.0,
                    MARGIN - TILE_PADDING[0],
                    MARGIN,
                ))
                .children(grid_rows(tiles.clone(), columns, ROW_GAP))
                .into_widget()
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

    /// One line saying nothing is installed, and the button that gets programs.
    fn nothing_installed(&self, resources: &ThemeResources) -> WidgetRef {
        let said = Text::new(words::NO_PROGRAMS)
            .text_align(TextAlign::Center)
            .style(TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary));
        let get = Button::text(
            words::GET_PROGRAMS,
            show(&self.library, LibraryPage::GetPrograms),
        )
        .style(ButtonStyle::Accent);
        Center::new()
            .child(
                Column::new()
                    .main_axis_size(MainAxisSize::Min)
                    .spacing(16.0)
                    .children([said.into_widget(), get.into_widget()]),
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
