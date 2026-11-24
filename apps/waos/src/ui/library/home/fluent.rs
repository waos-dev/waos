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

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BuildContext, Center, Color, ColoredBox, Column,
    CrossAxisAlignment, CustomScrollView, DecoratedBox, EdgeInsetsGeometry, Entity, Expanded,
    GlobalKey, Handle, IntoWidget, KeyedSubtree, LayoutBuilder, Listener, MainAxisSize, Padding,
    Row, SizedBox, SliverList, SliverPadding, SliverPersistentHeader,
    SliverPersistentHeaderDelegate, SliverPersistentHeaderDelegateRef, SliverToBoxAdapter, State,
    StateData, StatefulWidget, StatelessWidget, Text, TextAlign, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, InfoBadgeStyle, TextBlockStyle, ThemeResources,
    ThemeScope,
};

use super::motion::SlidesIntoPlace;
use super::{
    HomeEntry, IconTile, TileLook, badge_dot, grid_rows, home_entries, often_used_entries,
};
use crate::logic::icon_rows::{IconPlace, first_row};
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
/// How thick the line between the home's sections is.
const SECTION_LINE: f64 = 1.0;
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

/// The hairline between the apps used often and all of them, inset as the icons are.
#[derive(Debug)]
pub(crate) struct SectionLine {
    color: Color,
}

impl StatelessWidget for SectionLine {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 0.0))
            .child(ColoredBox::new(self.color).child(SizedBox::expand()))
            .into_widget()
    }
}

/// The [`SectionLine`] as a header of the home's scrolling: it scrolls up with the icons and
/// stays at the top of the grid once there, the icons passing beneath it.
#[derive(Debug)]
struct PinnedSectionLine {
    color: Color,
}

impl SliverPersistentHeaderDelegate for PinnedSectionLine {
    fn build(
        &self,
        _app: &mut App,
        _context: BuildContext,
        _shrink_offset: f64,
        _overlaps_content: bool,
    ) -> WidgetRef {
        SectionLine { color: self.color }.into_widget()
    }

    fn min_extent(&self) -> f64 {
        SECTION_LINE
    }

    fn max_extent(&self) -> f64 {
        SECTION_LINE
    }

    fn should_rebuild(&self, old: &dyn SliverPersistentHeaderDelegate) -> bool {
        old.as_any()
            .downcast_ref::<PinnedSectionLine>()
            .is_none_or(|old| old.color != self.color)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The home's icons, scrolling: the installed `apps`, in as many columns as the width holds,
/// under a line that stays at the top as they scroll beneath it; and over the line, where the
/// apps fill more than a row, a row of those `often_used`, which scrolls away, as macOS's Apps
/// shows its apps. Each icon slides to its place where the icons change (`motion`).
#[derive(Debug)]
struct IconGrid {
    /// Each installed app's icon, by its id, in the home's order.
    apps: Vec<(ProgramId, WidgetRef)>,
    /// The icons of the apps used often, the most used first.
    often_used: Vec<(ProgramId, WidgetRef)>,
    highlighted: Option<Highlighted>,
    line_color: Color,
}

/// Which of the home's two sections an icon is in: an app used often has one in each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    OftenUsed,
    Apps,
}

impl StatefulWidget for IconGrid {
    type State = IconGridState;

    fn create_state(&self) -> IconGridState {
        IconGridState {
            state: StateData::new(),
            keys: HashMap::new(),
            shown_before: false,
        }
    }
}

/// The key each icon's motion is kept under, so an icon moving to another row is the same icon.
struct IconGridState {
    state: StateData<IconGrid>,
    keys: HashMap<(Section, ProgramId), GlobalKey>,
    /// Whether the grid has shown icons before: only an icon that comes after fades in.
    shown_before: bool,
}

/// An icon of the grid with what its motion needs: the key it is kept under, and whether it was
/// not on the grid before.
#[derive(Clone)]
struct KeyedIcon {
    key: GlobalKey,
    appears: bool,
    icon: WidgetRef,
}

impl IconGridState {
    /// `icons` of `section`, each with its key, made for one the grid did not have.
    fn keyed(
        self: Handle<Self>,
        app: &mut App,
        section: Section,
        icons: &[(ProgramId, WidgetRef)],
    ) -> Vec<KeyedIcon> {
        let state = app.get_mut(self);
        let shown_before = state.shown_before;
        icons
            .iter()
            .map(|(id, icon)| {
                let mut appears = false;
                let key = state.keys.entry((section, id.clone())).or_insert_with(|| {
                    appears = shown_before;
                    GlobalKey::new()
                });
                KeyedIcon {
                    key: key.clone(),
                    appears,
                    icon: icon.clone(),
                }
            })
            .collect()
    }

    /// Lets go of the keys of icons the grid no longer shows, so one that comes back fades in.
    fn forget_icons_gone(self: Handle<Self>, app: &mut App) {
        let widget = self.widget(app);
        let shown: HashSet<(Section, ProgramId)> = widget
            .apps
            .iter()
            .map(|(id, _)| (Section::Apps, id.clone()))
            .chain(
                widget
                    .often_used
                    .iter()
                    .map(|(id, _)| (Section::OftenUsed, id.clone())),
            )
            .collect();
        let state = app.get_mut(self);
        state.keys.retain(|icon, _| shown.contains(icon));
        state.shown_before = true;
    }
}

impl State for IconGridState {
    type Widget = IconGrid;
    waft_widgets::state_accessors!();

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let (apps, often_used, highlighted, line_color) = {
            let widget = self.widget(app);
            (
                widget.apps.clone(),
                widget.often_used.clone(),
                widget.highlighted.clone(),
                widget.line_color,
            )
        };
        let apps = self.keyed(app, Section::Apps, &apps);
        let often_used = self.keyed(app, Section::OftenUsed, &often_used);
        self.forget_icons_gone(app);
        let line: SliverPersistentHeaderDelegateRef =
            Rc::new(PinnedSectionLine { color: line_color });
        LayoutBuilder::new(move |_app, _context, constraints| {
            let side = MARGIN - TILE_PADDING[0];
            let columns = columns_in(constraints.max_width);
            let cell_width = (constraints.max_width - 2.0 * side) / columns as f64;
            let first_row = first_row(&often_used, apps.len(), columns);
            let rows_over_the_grid = usize::from(!first_row.is_empty());
            // Each icon in its place, highlighted where the arrow keys are on it, sliding there
            // where its place changed.
            let placed = |icons: &[KeyedIcon], rows_above: usize| -> Vec<WidgetRef> {
                icons
                    .iter()
                    .enumerate()
                    .map(|(index, keyed)| {
                        let place = IconPlace {
                            row: index / columns,
                            column: index % columns,
                        };
                        let shown = IconPlace {
                            row: place.row + rows_above,
                            ..place
                        };
                        let icon = match &highlighted {
                            Some(Highlighted { place, key }) if *place == shown => {
                                KeyedSubtree::new(IconHighlight {
                                    child: keyed.icon.clone(),
                                })
                                .key(key.clone().into())
                                .into_widget()
                            }
                            _ => keyed.icon.clone(),
                        };
                        SlidesIntoPlace {
                            key: keyed.key.clone().into(),
                            place,
                            cell_width,
                            row_gap: ROW_GAP,
                            appears: keyed.appears,
                            child: icon,
                        }
                        .into_widget()
                    })
                    .collect()
            };
            let mut slivers = Vec::new();
            if !first_row.is_empty() {
                let row =
                    Column::new().children(grid_rows(placed(&first_row, 0), columns, ROW_GAP));
                slivers.push(
                    SliverPadding::new(EdgeInsetsGeometry::from_ltrb(
                        side, GRID_TOP, side, GRID_TOP,
                    ))
                    .sliver(SliverToBoxAdapter::new().child(row))
                    .into_widget(),
                );
            }
            slivers.push(
                SliverPersistentHeader::new(Rc::clone(&line))
                    .pinned(true)
                    .into_widget(),
            );
            // The room above the first row is the list's own, so the icons scroll up to the line
            // with no gap, where a gap outside the list would cut them short of it.
            slivers.push(
                SliverPadding::new(EdgeInsetsGeometry::from_ltrb(side, GRID_TOP, side, MARGIN))
                    .sliver(SliverList::list(grid_rows(
                        placed(&apps, rows_over_the_grid),
                        columns,
                        ROW_GAP,
                    )))
                    .into_widget(),
            );
            // An icon answers a press, as a button does, and shows nothing for the pointer
            // resting over it: WinUI's way to restyle a subtree's buttons.
            ThemeScope::overriding(CustomScrollView::new().slivers(slivers))
                .any(|resources| {
                    resources.subtle_button_background_pointer_over =
                        resources.subtle_button_background;
                })
                .into_widget()
        })
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

/// The icon the arrow keys are on: its place among the home's rows, and the key its highlight
/// is built under, by which whoever moved it scrolls it into sight.
#[derive(Clone, Debug)]
pub(crate) struct Highlighted {
    pub(crate) place: IconPlace,
    pub(crate) key: GlobalKey,
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
        let categories = library.categories();
        if !categories.is_empty() {
            let chosen = self.library.clone();
            children.push(category_chips(
                &categories,
                library.category(),
                MARGIN,
                move |app, category| {
                    chosen.update(app, |library, cx| library.show_category(category, cx));
                },
            ));
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
        IconGrid {
            apps: tiles_of(entries),
            often_used: tiles_of(often_used),
            highlighted: self.highlighted.clone(),
            line_color: resources.divider_stroke_color_default,
        }
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
