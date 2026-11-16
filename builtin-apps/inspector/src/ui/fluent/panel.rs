//! A live panel, as D1 draws it in DevTools' shape: a toolbar of the panel's filter and its kind
//! toggles, each with its count; the table of its rows, with the detail of the row selected beside
//! it; and a status bar saying whether it listens and what it heard. Every panel is drawn by this
//! one view from its rows' type, and its parts are a list top to bottom, so a panel can take a
//! track above its table without the view being reshaped.
//!
//! The invariant: the view keeps only the filter's field; what the panel holds and which filter
//! shows are the panel entity's.

use std::fmt;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded, Handle,
    IntoWidget, Listener, MainAxisSize, Padding, Ref, Row, SizedBox, State, StateData,
    StatefulWidget, TextEditingController, WidgetRef, Wrap,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, InfoBadge, InfoBadgeStyle, InfoBar,
    InfoBarSeverity, TextBlockStyle, TextBox, ThemeResources, ToggleButton,
};

use super::detail::DetailPane;
use super::table::Table;
use super::{GAP, MARGIN, caption, line, said, status_dot};
use crate::clock;
use crate::logic::panels::{Filter, PanelRow};
use crate::state::page::{Panels, Tab};
use crate::state::panel::{Heard, LivePanel, Log, set_filter};
use crate::state::programs::Seen;
use crate::words;

/// The width of the detail pane.
const DETAIL_WIDTH: f64 = 320.0;

/// The panel of `tab`, of the program `seen`; nothing for the report.
pub(super) fn panel_of(panels: &Panels, tab: Tab, seen: &Seen) -> WidgetRef {
    let running = seen.program.running;
    match tab {
        Tab::Report => SizedBox::new().into_widget(),
        Tab::Network => PanelView::of(&panels.network, running),
        Tab::Files => PanelView::of(&panels.files, running),
        Tab::Access => PanelView::of(&panels.access, running),
        Tab::Output => PanelView::of(&panels.output, running),
        Tab::InUse => PanelView::of(&panels.in_use, running),
    }
}

/// How many rows the panel of `tab` holds; none for the report, or a panel that never listened.
pub(super) fn heard_count(app: &App, panels: &Panels, tab: Tab) -> Option<usize> {
    fn count<L: Log>(app: &App, panel: &Entity<LivePanel<L>>) -> Option<usize> {
        let panel = panel.read(app);
        (panel.heard != Heard::Nothing).then(|| panel.rows.handle().item_count(app))
    }
    match tab {
        Tab::Report => None,
        Tab::Network => count(app, &panels.network),
        Tab::Files => count(app, &panels.files),
        Tab::Access => count(app, &panels.access),
        Tab::Output => count(app, &panels.output),
        Tab::InUse => count(app, &panels.in_use),
    }
}

/// A panel's view.
struct PanelView<L: Log> {
    panel: Entity<LivePanel<L>>,
    /// Whether its program runs now.
    running: bool,
}

impl<L: Log> fmt::Debug for PanelView<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PanelView")
            .field("running", &self.running)
            .finish()
    }
}

impl<L: Log> PanelView<L> {
    fn of(panel: &Entity<LivePanel<L>>, running: bool) -> WidgetRef {
        PanelView {
            panel: panel.clone(),
            running,
        }
        .into_widget()
    }
}

impl<L: Log> StatefulWidget for PanelView<L> {
    type State = PanelViewState<L>;

    fn create_state(&self) -> PanelViewState<L> {
        PanelViewState {
            state: StateData::new(),
            text: None,
        }
    }
}

struct PanelViewState<L: Log> {
    state: StateData<PanelView<L>>,
    /// What the filter's field holds.
    text: Option<Ref<TextEditingController>>,
}

impl<L: Log> State for PanelViewState<L> {
    type Widget = PanelView<L>;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let text = TextEditingController::new(app);
        app.get_mut(self).text = Some(text);
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let (panel, running) = {
            let widget = self.widget(app);
            (widget.panel.clone(), widget.running)
        };
        let text = app.get(self).text.clone().expect("made in `init_state`");
        let heard = panel.read(app).heard.clone();
        let mut parts = vec![toolbar(app, &panel, text, &resources)];
        if let Heard::Refused(why) = &heard {
            parts.push(
                Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 8.0))
                    .child(
                        InfoBar::new(true, |_app, _open| {})
                            .severity(InfoBarSeverity::Error)
                            .is_closable(false)
                            .title(words::NOT_STARTED)
                            .message(why.clone()),
                    )
                    .into_widget(),
            );
        }
        let body = if heard == Heard::Nothing {
            said(words::NOT_RUNNING, &resources)
        } else {
            rows_and_detail(app, &panel)
        };
        parts.push(Expanded::new(body).into_widget());
        parts.push(status_bar(app, &panel, running, &resources));
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(parts)
            .into_widget()
    }
}

/// The filter's field and the panel's kind toggles, each with how many rows it keeps.
fn toolbar<L: Log>(
    app: &App,
    panel: &Entity<LivePanel<L>>,
    text: Ref<TextEditingController>,
    resources: &ThemeResources,
) -> WidgetRef {
    let read = panel.read(app);
    let rows: Vec<&L::Row> = read.all(app).collect();
    let shown = read.filter;
    let filtering = panel.clone();
    let field = TextBox::new(text.erase())
        .placeholder_text(words::FILTER_PANEL)
        .text_changed(move |app, typed| {
            let filter = filtering.read(app).filter;
            set_filter(app, &filtering, filter, &typed);
        });
    let toggles = Filter::offered::<L::Row>().into_iter().map(|filter| {
        let kept = rows.iter().filter(|row| filter.keeps(**row, "")).count();
        let picking = panel.clone();
        // The refused are counted in a critical badge, so they read before the rest.
        let count = match filter {
            Filter::Refused if kept > 0 => InfoBadge::new()
                .value(i32::try_from(kept).unwrap_or(i32::MAX))
                .style(InfoBadgeStyle::Critical)
                .into_widget(),
            _ => waft::Text::new(kept.to_string()).into_widget(),
        };
        let label = Row::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(6.0)
            .children([waft::Text::new(filter.name()).into_widget(), count]);
        ToggleButton::new(Some(filter == shown), move |app, _checked| {
            let typed = picking.read(app).text.clone();
            set_filter(app, &picking, filter, &typed);
        })
        .content(label)
        .into_widget()
    });
    let items = vec![
        FontIcon::symbol(FluentSymbol::Search)
            .font_size(14.0)
            .foreground(resources.text_fill_color_secondary)
            .into_widget(),
        SizedBox::new().width(200.0).child(field).into_widget(),
        Expanded::new(
            Wrap::new()
                .spacing(6.0)
                .run_spacing(6.0)
                .children(toggles.collect::<Vec<_>>()),
        )
        .into_widget(),
    ];
    Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, GAP, MARGIN, GAP))
        .child(
            Row::new()
                .spacing(8.0)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(items),
        )
        .into_widget()
}

/// The table of the panel's rows, with the detail of the row selected beside it.
fn rows_and_detail<L: Log>(app: &App, panel: &Entity<LivePanel<L>>) -> WidgetRef {
    let read = panel.read(app);
    let detail = read.selected(app).map(|row| {
        let selection = read.selection.clone();
        SizedBox::new()
            .width(DETAIL_WIDTH)
            .child(DetailPane {
                detail: row.detail(),
                close: Listener::new(move |app| selection.handle().deselect_all(app)),
            })
            .into_widget()
    });
    let table = Table::<L::Row> {
        rows: read.rows.clone(),
        selection: Some(read.selection.clone()),
        empty: <L::Row as PanelRow>::NOTHING_YET,
        narrowed: detail.is_some(),
    };
    let mut parts = vec![Expanded::new(table).into_widget()];
    parts.extend(detail);
    Row::new()
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children(parts)
        .into_widget()
}

/// Whether the panel listens, and since when; how many rows it holds and how many WaOS refused;
/// and what its rows moved, where they say.
fn status_bar<L: Log>(
    app: &App,
    panel: &Entity<LivePanel<L>>,
    running: bool,
    resources: &ThemeResources,
) -> WidgetRef {
    let read = panel.read(app);
    let rows: Vec<&L::Row> = read.all(app).collect();
    let (listening, state) = match (&read.heard, running) {
        (Heard::Since(since), true) => (
            true,
            format!(
                "{} {} {}",
                words::LISTENING,
                words::SINCE_LOWER,
                clock::time_of_day(*since)
            ),
        ),
        (Heard::Since(_), false) => (
            false,
            format!("{} · {}", words::ENDED, words::SHOWING_LAST_RUN),
        ),
        (Heard::Nothing | Heard::Refused(_), _) => (false, words::NOT_LISTENING.to_owned()),
    };
    let refused = rows.iter().filter(|row| row.refused()).count();
    let mut items = vec![
        status_dot(listening, resources),
        line(
            &state,
            TextBlockStyle::Caption,
            resources.text_fill_color_primary,
            resources,
        ),
        caption(&words::heard(rows.len(), refused), resources),
    ];
    items.extend(<L::Row as PanelRow>::summed(&rows).map(|summed| caption(&summed, resources)));
    let clear = {
        let panel = panel.clone();
        Button::new(
            Row::new()
                .main_axis_size(MainAxisSize::Min)
                .spacing(6.0)
                .children([
                    FontIcon::symbol(FluentSymbol::Dismiss)
                        .font_size(12.0)
                        .into_widget(),
                    waft::Text::new(words::CLEAR).into_widget(),
                ]),
            Listener::new(move |app| {
                panel.update(app, |panel, cx| panel.clear(cx));
            }),
        )
        .style(ButtonStyle::Subtle)
    };
    items.push(Expanded::new(SizedBox::new()).into_widget());
    items.push(clear.into_widget());
    Padding::new(EdgeInsetsGeometry::from_ltrb(
        MARGIN,
        4.0,
        MARGIN - 8.0,
        4.0,
    ))
    .child(
        Row::new()
            .spacing(GAP)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(items),
    )
    .into_widget()
}
