//! The deck's tabs, on the main thread: the list of them (`logic::tab_list`, which has the rules
//! of what a tab is, their order, the current one and the overview), how far each program's tab
//! got, and what each program is told of its view. Opening a program gives it its tab, the home's
//! where the home is open, and reserves the tab's screen for its first request (`TabView`),
//! measured as a tab's page in the deck's view (`logic::tab_page`); once the program's first
//! request takes the screen, the tab shows the view it became, and once the program ends on its
//! own the tab keeps its view, which holds its last picture, and says how it ended, until the user
//! closes it. Until the program asks, however long it takes, the tab shows that it starts, and a
//! program that ends without asking leaves only how it ended. Each view the program asks for after
//! its first is a tab of its own, beside its others; each tab is a window (decision 6 of
//! `docs/proposals/2026-11-21-views-a-program-asks-for.md`): closing one takes its view away, and
//! closing a program's last tab stops the program without asking, as a phone's switcher ends an
//! app.
//!
//! The tabs know nothing of how the deck moves: whoever moves it says where it heads
//! (`show_page`, `set_overview_open`, `go_home`) and that it got there (`arrived`).
//!
//! The invariant, a program's presence: its view is on screen exactly while its page is in sight,
//! which is while the deck heads to its page or rests on it, and, for the page the deck last
//! rested on, until the deck arrives somewhere else, another page or the overview; it is focused
//! exactly while the deck rests on its page and the deck's view has the input focus. So nothing
//! changes while a finger drags the deck, a release that heads elsewhere puts the page headed to
//! on screen at once and takes the focus from the page left, and the arrival takes the page left
//! off screen and gives the page arrived at the focus. In the overview, at rest, no view is on
//! screen. A view holds the keyboard, its canvas having the input focus, only while the deck
//! rests on its page. A running tab's view measures the page at every change of the deck's view, and a tab
//! ended or closed tells its program nothing again.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::ui::{ViewFocusDirection, ViewFocusEvent, ViewFocusState, ViewRef};
use waft::{
    App, Context, Entity, FrameCallback, SchedulerBinding, WeakEntity, WidgetsBinding,
    WidgetsBindingObserver,
};
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{PlacedView, Presence, ProgramEnd, ProgramView, RunningProgram, ViewRequest};
use waos_package::ProgramId;
use waos_program_window::ShownView;

use super::tab_view::TabView;
use crate::logic::installed_runs::OpenedRun;
use crate::logic::program_places::TabRequest;
use crate::logic::runs::Runs;
use crate::logic::tab_list::{TabId, TabKind, TabList};
use crate::logic::tab_page::page_metrics;

pub(crate) struct Tabs {
    list: TabList,
    /// How far the program of each program's tab got.
    stages: HashMap<TabId, TabStage>,
    /// The Waft view the deck is in, once the platform has given it.
    view: Option<ViewRef>,
    /// Whether the deck's view has the input focus, as Waft last said; so until it says.
    focused: bool,
    /// The view each running tab shows, which closing the tab takes away from its program.
    views: HashMap<TabId, ProgramView>,
    /// The screen of each tab whose program has not asked for its first view yet, which that
    /// request takes.
    screens: HashMap<TabId, (TabView, PlacedView)>,
    /// The programs running, which a tab closed stops.
    runs: Arc<Runs>,
}

/// One tab, as the deck shows it.
pub(crate) struct Tab<'a> {
    pub(crate) id: TabId,
    pub(crate) content: TabContent<'a>,
}

/// What a tab holds.
pub(crate) enum TabContent<'a> {
    /// The home: the library's programs, to open one from.
    Home,
    /// A program, and how far it got.
    Program {
        id: &'a ProgramId,
        stage: &'a TabStage,
    },
}

pub(crate) enum TabStage {
    /// The program is loading.
    Starting,
    /// The program runs, and its tab's screen waits for its first request.
    Waiting,
    /// The program runs, its view shown by the tab.
    Running(Entity<ShownView>),
    /// The program ended on its own, `end` saying how; the tab keeps its view, which holds its
    /// last picture, until the user closes it, or none where it ended without asking for one.
    Ended {
        shown: Option<Entity<ShownView>>,
        end: ProgramEnd,
    },
}

impl Tabs {
    /// The home alone, closing a tab's program among `runs`, following what Waft reports of the
    /// deck's view from now on, and opening a tab for each of `requests`, a request of a program
    /// in the deck after its first.
    pub(crate) fn listen(
        app: &mut App,
        runs: Arc<Runs>,
        requests: UnboundedReceiver<TabRequest>,
    ) -> Entity<Tabs> {
        let tabs = app.new_entity(|_| Tabs {
            list: TabList::new(),
            stages: HashMap::new(),
            view: None,
            focused: true,
            views: HashMap::new(),
            screens: HashMap::new(),
            runs,
        });
        let reports = Rc::new(DeckViewReports {
            tabs: tabs.downgrade(),
        });
        WidgetsBinding::instance(app).add_observer(app, reports);
        answer_requests(app, &tabs, requests);
        tabs
    }

    /// `program` asked for a view: its first takes the screen of the tab it was opened in, and each
    /// later one is a tab of its own beside its others, which the deck heads to; the request fails
    /// where the program has no tab, or the deck can show none.
    fn view_requested(
        &mut self,
        program: RunningProgram,
        request: ViewRequest,
        cx: &mut Context<Tabs>,
    ) {
        let id = program.name().parse::<ProgramId>().ok();
        let waiting = id.as_ref().and_then(|id| self.list.tab_of(id));
        if let Some(tab) = waiting
            && let Some((tab_view, placed)) = self.screens.remove(&tab)
        {
            let view = program.add_view(request.id, placed);
            tracing::info!(program = program.name(), ?tab, "its first view, in its tab");
            self.show(tab, tab_view, program, view, cx);
            return;
        }
        let opened = id
            .and_then(|id| self.list.open_another_view(&id))
            .ok_or("the program has no tab to open another beside")
            .and_then(|tab| match self.tab_view(tab, cx) {
                Ok(starting) => Ok((tab, starting)),
                Err(why) => {
                    self.list.close(tab);
                    Err(why)
                }
            });
        let (tab, (tab_view, placed)) = match opened {
            Ok(opened) => opened,
            Err(why) => {
                tracing::warn!(program = program.name(), why, "no tab for its view");
                program.view_request_failed(request.id, ViewError::Failed(why.to_owned()));
                return;
            }
        };
        let view = program.add_view(request.id, placed);
        tracing::info!(
            program = program.name(),
            ?tab,
            "another view, in a tab of its own"
        );
        self.show(tab, tab_view, program, view, cx);
    }

    /// Brings the deck's window forward, as Android brings WaOS's own task forward while a
    /// window of WaOS's is in sight; nothing before the platform gives the deck its view.
    pub(crate) fn bring_forward(&self) {
        if let Some(view) = &self.view {
            view.host()
                .request_focus_change(ViewFocusState::Focused, ViewFocusDirection::Undefined);
        }
    }

    /// The deck is shown in `view`, which its tabs' pages are measured in.
    pub(crate) fn shown_in(&mut self, view: ViewRef) {
        self.view = Some(view);
    }

    /// Each tab, in order.
    pub(crate) fn tabs(&self) -> impl Iterator<Item = Tab<'_>> {
        self.list.tabs().iter().map(|listed| Tab {
            id: listed.id,
            content: match &listed.kind {
                TabKind::Home => TabContent::Home,
                TabKind::Program(id) => TabContent::Program {
                    id,
                    stage: self
                        .stages
                        .get(&listed.id)
                        .expect("a program's tab has a stage"),
                },
            },
        })
    }

    /// The tab the deck is on, or heads to.
    pub(crate) fn current_tab(&self) -> Tab<'_> {
        let current = self.list.current();
        self.tabs()
            .find(|tab| tab.id == current)
            .expect("the current tab is open")
    }

    /// The list: the tabs' order, the current one, the overview, and what is in sight.
    pub(crate) fn list(&self) -> &TabList {
        &self.list
    }

    /// Opens the program `id` in a tab, which the deck heads to, its screen waiting for the
    /// program's first request; answers whether it opened one, not where `id` has a tab already,
    /// which the deck heads to. Why not, where the deck has no view yet or the system no compositor
    /// to make the tab's canvas on.
    pub(crate) fn open(&mut self, id: &ProgramId, cx: &mut Context<Tabs>) -> Result<bool, String> {
        if let Some(tab) = self.list.tab_of(id) {
            self.show_page(tab, cx);
            return Ok(false);
        }
        let tab = self.list.open_program(id).tab();
        let starting = match self.tab_view(tab, cx) {
            Ok(starting) => starting,
            Err(why) => {
                self.list.program_did_not_start(tab);
                return Err(why.to_owned());
            }
        };
        self.stages.insert(tab, TabStage::Starting);
        self.screens.insert(tab, starting);
        self.changed(cx);
        Ok(true)
    }

    /// The screen of `tab`, measured as a page of the deck's view, with the view as the engine is
    /// handed it; why not, where the deck has no view yet or the system no compositor to make the
    /// tab's canvas on.
    fn tab_view(
        &self,
        tab: TabId,
        cx: &mut Context<Tabs>,
    ) -> Result<(TabView, PlacedView), &'static str> {
        let view = self
            .view
            .as_ref()
            .ok_or("WaOS has no screen to show the program on yet")?;
        let compositor = cx
            .platform()
            .compositor()
            .ok_or("this system shows no composition layers, which a program's view is")?;
        let metrics = page_metrics(&view.metrics());
        let bars = view.host().system_bars().is_some();
        Ok(TabView::make(
            &*compositor,
            &metrics,
            self.presence(tab),
            bars,
        ))
    }

    /// What starting the program of `id`'s tab came to: the program, or none where it could not
    /// start, and the tab is the home's again, or goes where a home is open. A program whose tab
    /// was closed meanwhile is stopped; one whose first request took its tab already shows there;
    /// otherwise its tab waits for that request, and says how it ended where it ends without one.
    /// A program that ran already is not this tab's: a tab still starting is treated as one that
    /// could not start.
    pub(crate) fn started(
        &mut self,
        id: &ProgramId,
        started: Option<OpenedRun>,
        cx: &mut Context<Tabs>,
    ) {
        let starting = self.starting(id);
        let program = match started {
            Some(OpenedRun::Started(program)) => program,
            Some(OpenedRun::RunningAlready(_)) | None => {
                if starting.is_some() {
                    tracing::info!(%id, "it did not start in its tab: the tab goes");
                }
                self.did_not_start(starting, cx);
                return;
            }
        };
        let Some(tab) = self.list.tab_of(id) else {
            tracing::info!(%id, "its tab was closed as it started: stopped");
            program.stop();
            return;
        };
        if starting == Some(tab) {
            tracing::info!(%id, "the program runs, its tab waiting for its first request");
            self.stages.insert(tab, TabStage::Waiting);
            end_while_waiting(cx, tab, &program);
            cx.notify();
        }
    }

    /// `tab` shows `view` of `program` on `tab_view`, its screen, from now on.
    fn show(
        &mut self,
        tab: TabId,
        tab_view: TabView,
        program: RunningProgram,
        view: ProgramView,
        cx: &mut Context<Tabs>,
    ) {
        let Some(waft_view) = self.view.clone() else {
            tracing::warn!(
                ?tab,
                "the deck lost its view as the program asked: the tab goes"
            );
            program.stop();
            self.did_not_start(Some(tab), cx);
            return;
        };
        self.views.insert(tab, view.clone());
        let shown = tab_view.show(
            cx,
            view,
            Rc::clone(&waft_view),
            page_metrics(&waft_view.metrics()),
            self.presence(tab),
        );
        tracing::info!(?tab, "the program shows in its tab");
        end_when_ended(cx, tab, &program, shown.downgrade());
        shown.update(cx, |shown, cx| shown.set_holds_keyboard(false, cx));
        self.stages.insert(tab, TabStage::Running(shown));
        self.restand_after_the_next_frame(cx);
        cx.notify();
    }

    /// Once the next frame has shown what changed, every running tab's program hears how its view
    /// stands. A view's canvas takes the keyboard only once it is in the frame, so a tab that
    /// just came to show its program gives it the keyboard then.
    fn restand_after_the_next_frame(&self, cx: &mut Context<Tabs>) {
        let tabs = cx.weak_entity();
        SchedulerBinding::add_post_frame_callback(
            cx,
            FrameCallback::new(move |app, _| {
                if let Some(tabs) = tabs.upgrade() {
                    tabs.update(app, |tabs, cx| tabs.restand(cx));
                }
            }),
            None,
        );
    }

    /// The deck heads to the page of `tab`.
    pub(crate) fn show_page(&mut self, tab: TabId, cx: &mut Context<Tabs>) {
        self.list.show_page(tab);
        self.changed(cx);
    }

    /// The deck heads to its overview, or back to the page of the current tab.
    pub(crate) fn set_overview_open(&mut self, open: bool, cx: &mut Context<Tabs>) {
        self.list.set_overview_open(open);
        self.changed(cx);
    }

    /// The deck heads to the home's page, the home made at the end where none is open; answers
    /// its tab.
    pub(crate) fn go_home(&mut self, cx: &mut Context<Tabs>) -> TabId {
        let home = self.list.go_home();
        self.changed(cx);
        home
    }

    /// The deck rests where it was heading.
    pub(crate) fn arrived(&mut self, cx: &mut Context<Tabs>) {
        if !self.list.has_arrived() {
            self.list.arrived();
            self.changed(cx);
        }
    }

    /// Closes `tab`, as a window closes: its program's view goes, and its program, where this was
    /// its last tab, is stopped without asking; the home alone stays.
    pub(crate) fn close(&mut self, tab: TabId, cx: &mut Context<Tabs>) {
        let Some(TabKind::Program(id)) = self.list.close(tab) else {
            self.changed(cx);
            return;
        };
        let stage = self.stages.remove(&tab);
        let view = self.views.remove(&tab);
        self.screens.remove(&tab);
        if let Some(TabStage::Running(shown)) = &stage {
            shown.read(cx).gone();
        }
        if let Some(TabStage::Running(_) | TabStage::Waiting) = stage {
            let last = self.list.tab_of(&id).is_none();
            let runs = Arc::clone(&self.runs);
            cx.spawn(async move |_cx| {
                let Some(program) = runs.running(&id).await else {
                    return;
                };
                match view {
                    Some(view) if !last => {
                        tracing::info!(%id, "one of its tabs was closed: its view goes");
                        program.remove_view(view.id());
                    }
                    _ => {
                        tracing::info!(%id, "its last tab was closed: stopped");
                        program.stop();
                    }
                }
            });
        }
        self.changed(cx);
    }

    /// Closes every tab of the program `id`, which stops it.
    pub(crate) fn close_program(&mut self, id: &ProgramId, cx: &mut Context<Tabs>) {
        while let Some(tab) = self.list.tab_of(id) {
            self.close(tab, cx);
        }
    }

    /// The tab of the program `id`, while the program starts.
    fn starting(&self, id: &ProgramId) -> Option<TabId> {
        let tab = self.list.tab_of(id)?;
        matches!(self.stages.get(&tab), Some(TabStage::Starting)).then_some(tab)
    }

    /// The program of `tab`, if any, could not start: the tab is the home again, or goes.
    fn did_not_start(&mut self, tab: Option<TabId>, cx: &mut Context<Tabs>) {
        let Some(tab) = tab else {
            return;
        };
        self.stages.remove(&tab);
        self.screens.remove(&tab);
        self.list.program_did_not_start(tab);
        self.changed(cx);
    }

    /// The program of `tab`, showing `shown`, ended on its own, `end` saying how.
    fn ended(&mut self, tab: TabId, shown: &Entity<ShownView>, end: ProgramEnd, app: &App) {
        if !matches!(self.stages.get(&tab), Some(TabStage::Running(running)) if running == shown) {
            return;
        }
        shown.read(app).gone();
        tracing::info!(?tab, ?end, "the program in a tab ended");
        self.stages.insert(
            tab,
            TabStage::Ended {
                shown: Some(shown.clone()),
                end,
            },
        );
    }

    /// The program of `tab` ended on its own before it asked for a view, `end` saying how.
    fn ended_without_a_view(&mut self, tab: TabId, end: ProgramEnd) {
        if !matches!(self.stages.get(&tab), Some(TabStage::Waiting)) {
            return;
        }
        tracing::info!(
            ?tab,
            ?end,
            "the program in a tab ended without asking for a view"
        );
        self.stages
            .insert(tab, TabStage::Ended { shown: None, end });
    }

    /// How the view of `tab`'s program stands now: the module's invariant.
    fn presence(&self, tab: TabId) -> Presence {
        Presence {
            on_screen: self.list.in_sight(tab),
            occluded: false,
            focused: self.focused && self.list.rests_on(tab),
        }
    }

    /// The list changed: every running program hears how its view stands now, and whoever shows
    /// the tabs hears of the change.
    fn changed(&mut self, cx: &mut Context<Tabs>) {
        self.restand(cx);
        cx.notify();
    }

    /// Every running tab's program hears how its view stands now, and its view holds the
    /// keyboard only while the deck rests on its page.
    fn restand(&self, app: &mut App) {
        for (tab, stage) in &self.stages {
            if let TabStage::Running(shown) = stage {
                let presence = self.presence(*tab);
                let rested_on = self.list.rests_on(*tab);
                shown.update(app, |shown, cx| {
                    shown.set_presence(presence);
                    shown.set_holds_keyboard(rested_on, cx);
                });
            }
        }
    }

    /// The deck's view changed its metrics: every running tab's page is measured again.
    fn metrics_changed(&mut self, cx: &mut Context<Tabs>) {
        let Some(view) = &self.view else {
            return;
        };
        let metrics = page_metrics(&view.metrics());
        for stage in self.stages.values() {
            if let TabStage::Running(shown) = stage {
                shown.update(cx, |shown, _| shown.set_metrics(metrics));
            }
        }
    }

    /// Waft says a view gained or lost the input focus: the deck's, the page it rests on hears it.
    fn view_focus_changed(&mut self, event: ViewFocusEvent, cx: &mut Context<Tabs>) {
        let Some(view) = &self.view else {
            return;
        };
        if view.id() != event.view_id {
            return;
        }
        self.focused = event.state == ViewFocusState::Focused;
        self.restand(cx);
    }
}

/// From now on each of `requests` is answered with a tab of `tabs`, on the main thread.
fn answer_requests(
    app: &mut App,
    tabs: &Entity<Tabs>,
    mut requests: UnboundedReceiver<TabRequest>,
) {
    let tabs = tabs.downgrade();
    app.spawn(async move |cx| {
        while let Some(TabRequest { program, request }) = requests.next().await {
            let answered = cx.update(|app| {
                let Some(tabs) = tabs.upgrade() else {
                    return false;
                };
                tabs.update(app, |tabs, cx| tabs.view_requested(program, request, cx));
                true
            });
            if !answered {
                return;
            }
        }
    });
}

/// Marks `tab` ended, with no picture, once `program` ends on its own while the tab waits for its
/// first request.
fn end_while_waiting(cx: &mut Context<Tabs>, tab: TabId, program: &RunningProgram) {
    let ended = program.ended();
    let tabs = cx.weak_entity();
    cx.spawn(async move |cx| {
        let end = ended.await;
        cx.update(|app| {
            if let Some(tabs) = tabs.upgrade() {
                tabs.update(app, |tabs, cx| {
                    tabs.ended_without_a_view(tab, end);
                    cx.notify();
                });
            }
        });
    });
}

/// Marks `tab` ended once `program` ends on its own, while the tab shows `shown`.
fn end_when_ended(
    cx: &mut Context<Tabs>,
    tab: TabId,
    program: &RunningProgram,
    shown: WeakEntity<ShownView>,
) {
    let ended = program.ended();
    let tabs = cx.weak_entity();
    cx.spawn(async move |cx| {
        let end = ended.await;
        cx.update(|app| {
            let (Some(tabs), Some(shown)) = (tabs.upgrade(), shown.upgrade()) else {
                return;
            };
            tabs.update(app, |tabs, cx| {
                tabs.ended(tab, &shown, end, cx);
                cx.notify();
            });
        });
    });
}

/// What Waft reports of the views, passed on to the tabs.
struct DeckViewReports {
    tabs: WeakEntity<Tabs>,
}

impl DeckViewReports {
    fn update(&self, app: &mut App, f: impl FnOnce(&mut Tabs, &mut Context<Tabs>)) {
        if let Some(tabs) = self.tabs.upgrade() {
            tabs.update(app, f);
        }
    }
}

impl WidgetsBindingObserver for DeckViewReports {
    fn did_change_metrics(&self, app: &mut App) {
        self.update(app, Tabs::metrics_changed);
    }

    fn did_change_view_focus(&self, app: &mut App, event: ViewFocusEvent) {
        self.update(app, |tabs, cx| tabs.view_focus_changed(event, cx));
    }
}
