//! The home's icons, scrolling, as both layouts show them: the installed apps in rows, under a
//! line; and over the line, where the apps fill more than a row, a row of those used often, as
//! macOS's Apps shows its apps. Each icon slides to its place where the icons change (`motion`).
//! What is each layout's is how the grid is set ([`GridLook`]): its columns, its margins, and
//! whether the line stays at the top as the icons scroll beneath it.
//!
//! The invariant: an icon keeps its motion's state under its app's key wherever its row is, so
//! an icon that moves is the same icon, and one the grid let go of fades in when it comes back.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use waft::{
    App, BuildContext, Color, ColoredBox, Column, CustomScrollView, EdgeInsetsGeometry, GlobalKey,
    Handle, IntoWidget, KeyedSubtree, LayoutBuilder, Padding, SizedBox, SliverList, SliverPadding,
    SliverPersistentHeader, SliverPersistentHeaderDelegate, SliverPersistentHeaderDelegateRef,
    SliverToBoxAdapter, State, StateData, StatefulWidget, StatelessWidget, WidgetRef,
};
use waos_package::ProgramId;

use super::grid_rows;
use super::motion::SlidesIntoPlace;
use crate::logic::icon_rows::{IconPlace, first_row};

/// How thick the line between the home's sections is.
const SECTION_LINE: f64 = 1.0;

/// How a layout sets the grid.
#[derive(Clone, Debug)]
pub(crate) struct GridLook {
    /// How many columns of icons a grid of a width has.
    pub(crate) columns: fn(f64) -> usize,
    /// How far in from each side the icons' cells start, and how far the section line.
    pub(crate) side: f64,
    pub(crate) line_side: f64,
    /// The room over the grid, which scrolls away with it: a bar the grid scrolls beneath.
    pub(crate) under_a_bar: f64,
    /// The room above each section's first row, between two rows, and under the last row.
    pub(crate) section_top: f64,
    pub(crate) row_gap: f64,
    pub(crate) bottom: f64,
    pub(crate) line: SectionLineKind,
    pub(crate) line_color: Color,
    /// What draws the icon the arrow keys are on; none where no keys move over the icons.
    pub(crate) highlight: Option<fn(WidgetRef) -> WidgetRef>,
}

/// Where the line between the home's sections is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionLineKind {
    /// Always there, under what is over the grid, and at the grid's top once the icons scroll
    /// beneath it: the desktop's, under its search row.
    Stays,
    /// Only between the apps used often and all of them, scrolling with them: a phone's, whose
    /// navigation bar has an edge of its own.
    BetweenSections,
}

/// The icon the arrow keys are on: its place among the home's rows, and the key its highlight
/// is built under, by which whoever moved it scrolls it into sight.
#[derive(Clone, Debug)]
pub(crate) struct Highlighted {
    pub(crate) place: IconPlace,
    pub(crate) key: GlobalKey,
}

/// The hairline between the apps used often and all of them, `side` in from each side.
#[derive(Debug)]
pub(crate) struct SectionLine {
    color: Color,
    side: f64,
}

impl StatelessWidget for SectionLine {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            self.side, 0.0, self.side, 0.0,
        ))
        .child(ColoredBox::new(self.color).child(SizedBox::expand()))
        .into_widget()
    }
}

/// The [`SectionLine`] as a header of the home's scrolling: it scrolls up with the icons and
/// stays at the top of the grid once there, the icons passing beneath it.
#[derive(Debug)]
struct PinnedSectionLine {
    color: Color,
    side: f64,
}

impl SliverPersistentHeaderDelegate for PinnedSectionLine {
    fn build(
        &self,
        _app: &mut App,
        _context: BuildContext,
        _shrink_offset: f64,
        _overlaps_content: bool,
    ) -> WidgetRef {
        SectionLine {
            color: self.color,
            side: self.side,
        }
        .into_widget()
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
            .is_none_or(|old| old.color != self.color || old.side != self.side)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The home's icons, scrolling: the installed `apps`, in as many columns as the look says,
/// under the section line; and over the line, where the apps fill more than a row, a row of
/// those `often_used`, which scrolls away.
#[derive(Debug)]
pub(crate) struct IconGrid {
    /// Each installed app's icon, by its id, in the home's order.
    pub(crate) apps: Vec<(ProgramId, WidgetRef)>,
    /// The icons of the apps used often, the most used first.
    pub(crate) often_used: Vec<(ProgramId, WidgetRef)>,
    /// What scrolls away over the icons, where the layout has something to say there.
    pub(crate) over: Option<WidgetRef>,
    pub(crate) highlighted: Option<Highlighted>,
    pub(crate) look: GridLook,
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
pub(crate) struct IconGridState {
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
        let (apps, often_used, over, highlighted, look) = {
            let widget = self.widget(app);
            (
                widget.apps.clone(),
                widget.often_used.clone(),
                widget.over.clone(),
                widget.highlighted.clone(),
                widget.look.clone(),
            )
        };
        let apps = self.keyed(app, Section::Apps, &apps);
        let often_used = self.keyed(app, Section::OftenUsed, &often_used);
        self.forget_icons_gone(app);
        LayoutBuilder::new(move |_app, _context, constraints| {
            let side = look.side;
            let columns = (look.columns)(constraints.max_width).max(1);
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
                        let icon = match (&highlighted, look.highlight) {
                            (Some(Highlighted { place, key }), Some(highlight))
                                if *place == shown =>
                            {
                                KeyedSubtree::new(highlight(keyed.icon.clone()))
                                    .key(key.clone().into())
                                    .into_widget()
                            }
                            _ => keyed.icon.clone(),
                        };
                        SlidesIntoPlace {
                            key: keyed.key.clone().into(),
                            place,
                            cell_width,
                            row_gap: look.row_gap,
                            appears: keyed.appears,
                            child: icon,
                        }
                        .into_widget()
                    })
                    .collect()
            };
            let mut slivers = Vec::new();
            // The room under a bar is the list's own, so the icons scroll beneath the bar.
            let mut over_the_grid = look.under_a_bar;
            if let Some(over) = &over {
                slivers.push(
                    SliverPadding::new(EdgeInsetsGeometry::from_ltrb(0.0, over_the_grid, 0.0, 0.0))
                        .sliver(SliverToBoxAdapter::new().child(over.clone()))
                        .into_widget(),
                );
                over_the_grid = 0.0;
            }
            if !first_row.is_empty() {
                let row =
                    Column::new().children(grid_rows(placed(&first_row, 0), columns, look.row_gap));
                slivers.push(
                    SliverPadding::new(EdgeInsetsGeometry::from_ltrb(
                        side,
                        over_the_grid + look.section_top,
                        side,
                        look.section_top,
                    ))
                    .sliver(SliverToBoxAdapter::new().child(row))
                    .into_widget(),
                );
                over_the_grid = 0.0;
            }
            match look.line {
                SectionLineKind::Stays => {
                    let line: SliverPersistentHeaderDelegateRef = Rc::new(PinnedSectionLine {
                        color: look.line_color,
                        side: look.line_side,
                    });
                    slivers.push(SliverPersistentHeader::new(line).pinned(true).into_widget());
                }
                SectionLineKind::BetweenSections if !first_row.is_empty() => {
                    let line = SectionLine {
                        color: look.line_color,
                        side: look.line_side,
                    };
                    slivers.push(
                        SliverToBoxAdapter::new()
                            .child(SizedBox::new().height(SECTION_LINE).child(line))
                            .into_widget(),
                    );
                }
                SectionLineKind::BetweenSections => {}
            }
            // The room above the first row is the list's own, so the icons scroll up to the line
            // with no gap, where a gap outside the list would cut them short of it.
            slivers.push(
                SliverPadding::new(EdgeInsetsGeometry::from_ltrb(
                    side,
                    over_the_grid + look.section_top,
                    side,
                    look.bottom,
                ))
                .sliver(SliverList::list(grid_rows(
                    placed(&apps, rows_over_the_grid),
                    columns,
                    look.row_gap,
                )))
                .into_widget(),
            );
            CustomScrollView::new().slivers(slivers).into_widget()
        })
        .into_widget()
    }
}
