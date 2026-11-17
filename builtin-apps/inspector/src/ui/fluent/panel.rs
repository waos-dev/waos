//! A live panel, as D1 draws it in DevTools' shape: a toolbar of the panel's filter and its kind
//! toggles, each with its count; the table of its rows, with the detail of the row selected beside
//! it; and a status bar saying whether it listens and what it heard. Every panel is drawn by this
//! one view from its rows' type, and its parts are a list top to bottom, so a panel can take a
//! track above its table without the view being reshaped.
//!
//! The invariant: the view keeps only the filter's field; what the panel holds and which filter
//! shows are the panel entity's.

use std::fmt;
use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded, Handle,
    IntoWidget, Listenable, ListenableBuilder, Listener, MainAxisSize, Padding, Ref, Row, SizedBox,
    State, StateData, StatefulWidget, TextEditingController, WidgetRef, Wrap,
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
        Tab::Report | Tab::Storage => SizedBox::new().into_widget(),
        Tab::Network => PanelView::of(&panels.network, running),
        Tab::Files => PanelView::of(&panels.files, running),
        Tab::Access => PanelView::of(&panels.access, running),
        Tab::Output => PanelView::of(&panels.output, running),
        Tab::InUse => PanelView::of(&panels.in_use, running),
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

/// The table of the panel's rows, with the detail of the row selected beside it, drawn again as
/// the selection changes; the table narrows while the detail shows.
fn rows_and_detail<L: Log>(app: &App, panel: &Entity<LivePanel<L>>) -> WidgetRef {
    let selection = panel.read(app).selection.clone();
    let shown = panel.clone();
    ListenableBuilder::new(
        Rc::new(selection.clone()) as Rc<dyn Listenable>,
        move |app, _context, _child| {
            let read = shown.read(app);
            let detail = read.selected(app).map(|row| {
                let selection = selection.clone();
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
        },
    )
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
        caption(
            &words::heard(rows.len(), <L::Row as PanelRow>::COUNTED_AS, refused),
            resources,
        ),
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::time::Duration;

    use waft::ui::TargetPlatform;
    use waft::{
        App, AppCell, BuildContext, Entity, IntoWidget, PaintingBinding, SchedulerBinding,
        SizedBox, StatelessWidget, WidgetRef, WidgetsApp, run_app,
    };
    use waft_test::{IMPLICIT_VIEW_ID, TestPlatform, TestView};
    use waft_winui::{AccentPalette, Theme, ThemeScope, install_icon_font};
    use wasdk::inspect::trace::{DeviceEvent, DeviceOpened, DeviceUse, Verdict};
    use wasdk::system_clock::Instant;

    use super::*;
    use crate::logic::panels::access::{AccessLog, Capturing};

    /// A view on a macOS host, which lays the app out.
    fn host() -> TestPlatform {
        TestPlatform::new()
            .on(TargetPlatform::MacOS)
            .with_view(Rc::new(TestView::new(800.0, 600.0)))
    }

    /// One frame, as the host draws it.
    fn pump(cell: &Rc<AppCell>, time: &mut Duration) {
        *time += Duration::from_millis(20);
        let mut app = cell.borrow_mut();
        let views = [IMPLICIT_VIEW_ID];
        SchedulerBinding::handle_begin_frame(&mut app, &views, Some(*time));
        app.drain_microtasks();
        SchedulerBinding::handle_draw_frame(&mut app, &views);
        app.drain_microtasks();
    }

    /// The panel, or nothing, as showing another tab and coming back does.
    #[derive(Debug)]
    struct Shown {
        panel: Entity<LivePanel<AccessLog>>,
        showing: Entity<bool>,
    }

    impl StatelessWidget for Shown {
        fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
            if *self.showing.read(app) {
                PanelView::of(&self.panel, true)
            } else {
                SizedBox::new().into_widget()
            }
        }
    }

    /// The Access card on the report shows the same panel again, as switching tabs does. The
    /// panel's test keeps its row selected; Waft's `TableView`, binding that selection when the
    /// table is made again, clears the row and tells the `ListenableBuilder` above it during the
    /// build, which marks it dirty and panics. This passes only once Waft keeps a selection a new
    /// table binds over the same source.
    #[test]
    fn a_selected_access_row_survives_showing_the_report_and_coming_back() {
        let cell = AppCell::with_platform(Rc::new(host()));
        let (panel, showing) = {
            let mut app = cell.borrow_mut();
            PaintingBinding::instance(&mut app).install_platform_fonts(&mut app);
            install_icon_font(&mut app);
            let panel = LivePanel::<AccessLog>::new(&mut app);
            let showing = app.new_entity(|_| true);
            let at = Instant {
                seconds: 1,
                nanoseconds: 0,
            };
            panel.update(&mut app, |panel, cx| {
                panel.begin(cx);
            });
            panel.update(&mut app, |panel, cx| {
                panel.tell(cx, |log, rows| {
                    log.add_capture(
                        rows,
                        Capturing::Camera,
                        &DeviceEvent {
                            at,
                            instance: 0,
                            dropped_before: 0,
                            kind: DeviceUse::Opened(DeviceOpened {
                                camera_id: None,
                                simulated: false,
                            }),
                            verdict: Verdict::Allowed,
                        },
                    );
                });
            });
            let root_panel = panel.clone();
            let root_showing = showing.clone();
            run_app(
                &mut app,
                WidgetsApp::new(AccentPalette::default().base)
                    .builder(move |_app, _context, _child| {
                        ThemeScope::new(
                            Theme::Light,
                            Shown {
                                panel: root_panel.clone(),
                                showing: root_showing.clone(),
                            }
                            .into_widget(),
                        )
                        .into_widget()
                    })
                    .into_widget(),
            );
            (panel, showing)
        };
        let mut time = Duration::ZERO;
        cell.elapse(Duration::ZERO);
        pump(&cell, &mut time);

        let selection = panel.read(&cell.borrow()).selection.handle();
        selection.select(&mut cell.borrow_mut(), 0);
        assert_eq!(
            selection.selected_index(&cell.borrow()),
            Some(0),
            "a row is selected"
        );
        pump(&cell, &mut time);

        showing.update(&mut cell.borrow_mut(), |showing, cx| {
            *showing = false;
            cx.notify();
        });
        pump(&cell, &mut time);

        showing.update(&mut cell.borrow_mut(), |showing, cx| {
            *showing = true;
            cx.notify();
        });
        pump(&cell, &mut time);
        assert_eq!(
            selection.selected_index(&cell.borrow()),
            Some(0),
            "the row is still selected when the panel shows again"
        );
    }
}
