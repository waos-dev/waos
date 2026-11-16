//! The deck on a phone's screen: every tab's page, the bar along the bottom that names the page
//! and swipes between pages, and the overview the pages shrink into as cards.
//!
//! It is all one stack, drawn at every frame from the deck's animated values by
//! `logic::deck_geometry`: where the strip of pages is, how far the overview's grid is scrolled,
//! and each tab's own, its card's slot and throw, its flight between its page and its card, and
//! how far a finger carries its page. The values are unbounded controllers that fingers drag and
//! springs settle (`springs` says why none has a duration). What the tabs are, which is current
//! and whether the overview is open is the tabs' (`state::deck::tabs`): the deck changes it only
//! through the tabs' calls, and follows it whoever changed it. Each gesture is a module of its
//! own, as methods of the deck's state:
//!
//! - `strip`: swiping the bar sideways between pages, and onto the home past the last;
//! - `overview_transition`: dragging the bar upward, or tapping the tabs button, to take the
//!   current page into flight to its card, over a grid that stands still, and a card flying back
//!   to its page;
//! - `grid_scroll`: scrolling the overview's grid;
//! - `cards`: tapping a card, and throwing one away to close its tab, the others moving into its
//!   place.
//!
//! The system's Back is answered by `back`, through the same calls as the deck's own controls.
//!
//! On the home the bar's pill is a search field (`search_pill`), which a gesture that starts on
//! the bar, or the deck heading elsewhere, takes the keyboard from.
//!
//! One rule settles the deck: wherever the tabs say it heads, the strip springs to the current
//! tab's page and every tab's flight to its page or its card, each from where it is at the speed
//! it has, unless a finger holds it. A gesture holds a value while its finger is down (`grip`),
//! and ends by saying where the deck heads, by where the release would carry the value
//! (`projection`), and letting go; nothing waits for a motion to be over.
//! A change of where the deck heads that no gesture of its own made, a program opened from outside
//! or the current tab closed, takes every value out of the fingers' hands, and so does a change of
//! the view's size: the finger still down does nothing more.
//!
//! While the grid stands, a change of the current tab that the strip's own swipe did not make
//! puts the strip on the new tab at once: no card in the grid moves by it, and a page still in
//! flight is kept where it is. With the grid laid down the strip glides there past the pages
//! between.
//!
//! Touches follow where a page is going, not where it is drawn: the pages take them while the
//! deck rests on a page or the current page flies to it with no finger on it, each page at the
//! place it lands on the strip (`frames`), and the touch layer over them (`touch_layer`) takes
//! them otherwise.
//!
//! The deck tells the tabs it arrived once nothing holds or moves the strip or a flight and they
//! are where the tabs say the deck heads, which is when the tabs give the page arrived at the
//! focus.
//!
//! What the library asks or says shows as an alert over the whole deck, the home or a program's
//! tab alike.
//!
//! The invariants: a tab's page is laid out at the box `logic::tab_page` measures, the size its
//! program is told, and only ever moved, scaled and clipped; a program's page takes every touch
//! on it, the deck taking none but what its bar offers; and every child of the deck's stack is
//! keyed, so that one coming or going, or the tabs painted in another order, never builds
//! another again: a gesture lives in the bar's or the touch layer's detectors, and ends with
//! them.

mod axis_drag;
mod back;
mod bar;
mod card_chrome;
mod cards;
mod frames;
mod grid_scroll;
mod grip;
mod overscroll;
mod overview_toolbar;
mod overview_transition;
mod pages;
mod projection;
mod search_pill;
mod springs;
mod strip;
#[cfg(test)]
mod tests;
mod touch_layer;
mod words;

use std::collections::{HashMap, HashSet};

use waft::{
    Animation, AnimationBehavior, AnimationController, App, BuildContext, Clip, ColoredBox,
    EdgeInsets, Entity, FrameCallback, Handle, IntoWidget, KeyedSubtree, Listener, MediaQuery,
    MediaQueryData, Offset, Ref, SchedulerBinding, Size, Stack, StackFit, State, StateData,
    StatefulWidget, StatelessWidget, Subscription, Ticker, TickerCallback, TickerProviderObject,
    TickerProviderStateMixin, TickerProviderStateMixinData, ValueKey, WidgetRef,
    WidgetsBindingObserverRef,
};
use waft_cupertino::{CupertinoApp, CupertinoColors};

use crate::logic::deck_geometry::{CardPosition, DeckLayout, DeckPosition};
use crate::logic::tab_list::TabId;
use crate::logic::tab_page::beneath_the_bar;
use crate::state::deck::tabs::{TabContent, Tabs};
use crate::state::library::Library;
use crate::state::recordings::Recordings;
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::questions::cupertino::CupertinoAsked;
use cards::CardMotions;
use frames::PlacedTab;
use grip::Grip;
use overview_transition::HeldPage;
use pages::{NamedProgram, PageShown, TabPage};
use search_pill::SearchField;
use strip::strip_among;

/// The deck, over the library and its tabs, under an app of its own.
#[derive(Clone, Debug)]
pub(crate) struct Deck {
    pub(crate) tabs: Entity<Tabs>,
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
}

impl StatelessWidget for Deck {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        CupertinoApp::new()
            .title("WaOS")
            .home(DeckView {
                tabs: self.tabs.clone(),
                library: self.library.clone(),
                recordings: self.recordings.clone(),
            })
            .into_widget()
    }
}

/// The deck's pages, its bar and its overview, filling the view.
#[derive(Debug)]
pub(crate) struct DeckView {
    tabs: Entity<Tabs>,
    library: Entity<Library>,
    recordings: Entity<Recordings>,
}

impl StatefulWidget for DeckView {
    type State = DeckViewState;

    fn create_state(&self) -> DeckViewState {
        DeckViewState {
            state: StateData::new(),
            ticker_provider: TickerProviderStateMixinData::new(),
            motions: None,
            strip_grip: Grip::Free,
            strip_target: None,
            overview_grip: Grip::Free,
            held_page: None,
            grid_grip: Grip::Free,
            grid_stands: false,
            cards: HashMap::new(),
            leaving: HashSet::new(),
            layout: None,
            followed: Followed::default(),
            pages: HashMap::new(),
            search: None,
            tab_changes: None,
            changing_tabs: false,
            back: None,
        }
    }
}

/// What keys a tab's page among the deck's children. The home's is the same page wherever it is,
/// past the last tab or in a tab of its own, so the home opening where it was dragged into view
/// builds nothing; a program's is its tab's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FrameKey {
    Home,
    Program(TabId),
}

/// What keys a child of the deck's stack that is no tab's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DeckPart {
    Background,
    TouchLayer,
    Toolbar,
    Bar,
}

impl DeckPart {
    fn keyed(self, part: WidgetRef) -> WidgetRef {
        KeyedSubtree::new(part)
            .key(ValueKey::new(self).into())
            .into_widget()
    }
}

/// One page the deck draws: a tab's, or the home's past the last tab while no home is open.
struct ShownTab {
    /// Its tab; none for the page past the last tab.
    tab: Option<TabId>,
    key: FrameKey,
    shown: PageShown,
    /// The tab's program; none for the home.
    program: Option<NamedProgram>,
}

/// A tab's page, with what it was built for, so it is built again only when the tab shows
/// something else.
struct BuiltPage {
    shown: PageShown,
    program: Option<NamedProgram>,
    widget: WidgetRef,
}

/// What the deck last saw of the tabs, which it tells what changed by.
#[derive(Clone, Debug, Default, PartialEq)]
struct Followed {
    order: Vec<TabId>,
    current: Option<TabId>,
    overview_open: bool,
    /// The home's tab, where one is open.
    home: Option<TabId>,
}

impl Followed {
    fn of(tabs: &Tabs) -> Followed {
        let list = tabs.list();
        Followed {
            order: list.tabs().iter().map(|tab| tab.id).collect(),
            current: Some(list.current()),
            overview_open: list.overview_open(),
            home: list.home(),
        }
    }

    /// Where the deck heads: the current tab, and its page or the overview.
    fn destination(&self) -> (Option<TabId>, bool) {
        (self.current, self.overview_open)
    }

    fn current_index(&self) -> usize {
        self.current
            .and_then(|current| self.order.iter().position(|tab| *tab == current))
            .unwrap_or(0)
    }
}

/// How the strip gets to the current tab's page when the tabs change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StripFollows {
    /// It springs there from where it is, past the pages between: the strip's own swipe.
    Gliding,
    /// It is there at once while the grid stands, where the strip moves no card and a page in
    /// flight is kept where it is; with the grid laid down it glides.
    Unseen,
}

/// The deck's own animated values.
struct DeckMotions {
    /// Where the strip of pages is, in pages.
    strip: Ref<AnimationController>,
    /// How far the overview's grid is scrolled, in points.
    grid_scroll: Ref<AnimationController>,
}

pub(crate) struct DeckViewState {
    state: StateData<DeckView>,
    ticker_provider: TickerProviderStateMixinData,
    motions: Option<DeckMotions>,
    strip_grip: Grip,
    /// The page the strip's spring is settling on, while it is.
    strip_target: Option<f64>,
    overview_grip: Grip,
    /// The hold of the page a finger drags into the overview, while it drags.
    held_page: Option<HeldPage>,
    grid_grip: Grip,
    /// Whether the overview's grid is stood up: from the moment a flight to the overview begins
    /// until the deck rests on a page again, every tab but the one flying to its page is its
    /// card in the grid; while it is laid down, every tab is its page on the strip.
    grid_stands: bool,
    /// Each tab's card.
    cards: HashMap<TabId, CardMotions>,
    /// The tabs whose cards were thrown away and are flying off, their tabs closing once gone.
    leaving: HashSet<TabId>,
    /// The view as the last build measured it, which the gestures that follow measure against;
    /// none before the first build.
    layout: Option<DeckLayout>,
    followed: Followed,
    pages: HashMap<FrameKey, BuiltPage>,
    /// The home's search field, which outlives the pill that shows it.
    search: Option<SearchField>,
    /// Keeps the deck following the tabs.
    tab_changes: Option<Subscription>,
    /// Whether the deck itself is changing the tabs now, and follows them once it has.
    changing_tabs: bool,
    /// What hears the system's Back for the deck, while it is shown.
    back: Option<WidgetsBindingObserverRef>,
}

impl DeckViewState {
    fn tabs(self: Handle<Self>, app: &App) -> Entity<Tabs> {
        self.widget(app).tabs.clone()
    }

    fn motions(self: Handle<Self>, app: &App) -> &DeckMotions {
        app.get(self)
            .motions
            .as_ref()
            .expect("created in init_state")
    }

    fn strip(self: Handle<Self>, app: &App) -> Handle<AnimationController> {
        self.motions(app).strip.handle()
    }

    fn grid_scroll(self: Handle<Self>, app: &App) -> Handle<AnimationController> {
        self.motions(app).grid_scroll.handle()
    }

    /// The view as the last build measured it; a phone's until the first.
    fn layout(self: Handle<Self>, app: &App) -> DeckLayout {
        app.get(self)
            .layout
            .unwrap_or_else(|| DeckLayout::new(Size::new(390.0, 844.0), 0.0, 0.0))
    }

    fn redraw(self: Handle<Self>, app: &mut App) {
        if self.mounted(app) {
            self.set_state(app, |_| {});
        }
    }

    /// An unbounded controller at `value` that redraws the deck whenever it moves.
    fn new_motion(self: Handle<Self>, app: &mut App, value: f64) -> Ref<AnimationController> {
        let controller = AnimationController::unbounded(
            app,
            value,
            None,
            None,
            AnimationBehavior::Preserve,
            self,
        );
        controller.add_listener(app, Listener::new(move |app| self.redraw(app)));
        controller
    }

    /// Where the deck's animated values are now.
    fn position(self: Handle<Self>, app: &App) -> DeckPosition {
        let motions = self.motions(app);
        DeckPosition {
            strip: motions.strip.value(app),
            grid_scroll: motions.grid_scroll.value(app),
        }
    }

    /// Where the values of each of `tabs` are now, in their order.
    fn card_positions_of(self: Handle<Self>, app: &App, tabs: &[TabId]) -> Vec<CardPosition> {
        let state = app.get(self);
        tabs.iter()
            .enumerate()
            .map(|(index, tab)| match state.cards.get(tab) {
                Some(card) => CardPosition {
                    slot: card.slot.value(app),
                    throw_offset: card.throw.value(app),
                    flight: card.flight.value(app),
                    carried: Offset::new(card.carry[0].value(app), card.carry[1].value(app)),
                },
                None if state.grid_stands => CardPosition::card(index),
                None => CardPosition::page(index),
            })
            .collect()
    }

    /// Where each tab's card is now, in the tabs' order.
    fn card_positions(self: Handle<Self>, app: &App) -> Vec<CardPosition> {
        self.card_positions_of(app, &app.get(self).followed.order)
    }

    /// Whether the page of `tab`, none for the page past the last tab, takes the touches on it:
    /// every page while the grid is laid down, every tab its page on the strip; while the grid
    /// stands, the current tab's alone, while its flight heads to its page with no finger on it.
    fn page_takes_touches(self: Handle<Self>, app: &App, tab: Option<TabId>) -> bool {
        let state = app.get(self);
        if !state.grid_stands {
            return true;
        }
        tab.is_some() && tab == state.followed.current && self.current_page_lands_untouched(app)
    }

    /// Whether the touch layer takes the touches over the pages: whenever the current tab's page
    /// does not.
    fn touch_layer_takes_touches(self: Handle<Self>, app: &App) -> bool {
        !self.page_takes_touches(app, app.get(self).followed.current)
    }

    /// Changes the tabs as `change` does, and follows them at once, the strip as `strip_follows`
    /// says.
    fn change_tabs(
        self: Handle<Self>,
        app: &mut App,
        strip_follows: StripFollows,
        change: impl FnOnce(&mut Tabs, &mut waft::Context<Tabs>),
    ) {
        app.get_mut(self).changing_tabs = true;
        self.tabs(app).update(app, change);
        app.get_mut(self).changing_tabs = false;
        self.follow_tabs(app, strip_follows);
    }

    /// The tabs changed, and not by the deck's own hand: where they send the deck elsewhere,
    /// every value is taken out of the fingers' hands, and the deck follows.
    fn tabs_changed_from_outside(self: Handle<Self>, app: &mut App) {
        if app.get(self).changing_tabs {
            return;
        }
        let now = Followed::of(self.tabs(app).read(app));
        if now.destination() != app.get(self).followed.destination() {
            self.drop_every_grip(app);
        }
        self.follow_tabs(app, StripFollows::Unseen);
    }

    /// Follows the tabs as they are now: the search field gives the keyboard up where the deck
    /// heads elsewhere, and is emptied where the home's tab is another or gone; every tab has a
    /// card and no other does, the strip goes
    /// to the current tab's page as `strip_follows` says, keeping the pages it shows where they
    /// are whatever tab came or went, the cards move to their slots, and the deck settles where
    /// the tabs say it heads.
    fn follow_tabs(self: Handle<Self>, app: &mut App, strip_follows: StripFollows) {
        let now = Followed::of(self.tabs(app).read(app));
        let before = std::mem::replace(&mut app.get_mut(self).followed, now.clone());
        if now.destination() != before.destination() {
            self.leave_search(app);
        }
        if now.home != before.home {
            self.clear_search(app);
        }
        self.provide_cards(app);
        if now.overview_open {
            self.stand_grid(app);
        }
        let grid_stands = app.get(self).grid_stands;
        let current_changed = now.current != before.current;
        if strip_follows == StripFollows::Unseen && grid_stands && current_changed {
            self.keeping_pages_in_flight_in_place(app, Self::put_strip_on_current_page);
        } else {
            self.move_strip_with_its_page(app, &before.order, &now.order);
        }
        self.move_cards_to_their_slots(app);
        self.keep_grid_in_range(app);
        self.settle_strip(app, None);
        self.settle_flights(app);
        self.redraw(app);
    }

    /// Gives every tab its card's motions, in its slot among the cards staying, where it has none
    /// yet, and keeps nothing of a tab that is gone.
    fn provide_cards(self: Handle<Self>, app: &mut App) {
        let order = app.get(self).followed.order.clone();
        let gone: Vec<TabId> = app
            .get(self)
            .cards
            .keys()
            .filter(|tab| !order.contains(tab))
            .copied()
            .collect();
        for tab in gone {
            self.forget_card(app, tab);
        }
        let grid_stands = app.get(self).grid_stands;
        for (slot, tab) in self.cards_staying(app).into_iter().enumerate() {
            if app.get(self).cards.contains_key(&tab) {
                continue;
            }
            let card = CardMotions {
                slot: self.new_motion(app, slot as f64),
                slot_heading: slot as f64,
                throw: self.new_motion(app, 0.0),
                throw_grip: Grip::Free,
                caught_in_flight: false,
                flight: self.new_motion(app, if grid_stands { 1.0 } else { 0.0 }),
                flight_heading: None,
                carry: [self.new_motion(app, 0.0), self.new_motion(app, 0.0)],
                carry_settling: [false; 2],
            };
            app.get_mut(self).cards.insert(tab, card);
        }
    }

    /// Drops the card of a tab that is gone.
    fn forget_card(self: Handle<Self>, app: &mut App, tab: TabId) {
        app.get_mut(self).leaving.remove(&tab);
        if let Some(card) = app.get_mut(self).cards.remove(&tab) {
            let [across, down] = card.carry;
            for motion in [card.slot, card.throw, card.flight, across, down] {
                motion.dispose(app);
            }
        }
    }

    /// Puts the strip on the current tab's page at once.
    fn put_strip_on_current_page(self: Handle<Self>, app: &mut App) {
        let target = app.get(self).followed.current_index() as f64;
        self.stop_strip(app);
        self.strip(app).set_value(app, target);
    }

    /// Keeps the page the strip shows in view as the tabs in `before` become those in `now`,
    /// and its spring going from there.
    fn move_strip_with_its_page(
        self: Handle<Self>,
        app: &mut App,
        before: &[TabId],
        now: &[TabId],
    ) {
        let strip = self.strip(app);
        let was = strip.value(app);
        let moved = strip_among(was, before, now);
        if moved == was {
            return;
        }
        let velocity = strip.velocity(app);
        let was_settling = app.get(self).strip_target.is_some();
        self.stop_strip(app);
        strip.set_value(app, moved);
        if was_settling {
            self.settle_strip(app, Some(velocity));
        }
    }

    /// Stops the strip where it is.
    fn stop_strip(self: Handle<Self>, app: &mut App) {
        self.strip(app).stop(app, true);
        app.get_mut(self).strip_target = None;
    }

    /// Lets the strip settle on the current tab's page, from where it is, at `velocity`, in pages
    /// a second, or at the speed it has; nothing while a finger has it.
    fn settle_strip(self: Handle<Self>, app: &mut App, velocity: Option<f64>) {
        if !app.get(self).strip_grip.is_free() {
            return;
        }
        let target = app.get(self).followed.current_index() as f64;
        let strip = self.strip(app);
        let heading_there = app.get(self).strip_target == Some(target);
        if (heading_there && velocity.is_none()) || (strip.value(app) == target && !heading_there) {
            self.check_arrival(app);
            return;
        }
        let velocity = velocity.unwrap_or_else(|| strip.velocity(app));
        app.get_mut(self).strip_target = Some(target);
        let spacing = self.layout(app).page_spacing();
        springs::settle(app, strip, target, velocity, springs::page_swipe(), spacing)
            .when_complete(app, Listener::new(move |app| self.strip_settled(app)));
    }

    fn strip_settled(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).strip_target = None;
        self.check_arrival(app);
    }

    /// Tells the tabs the deck arrived, once nothing holds or moves the strip or a flight, the
    /// strip is on the current tab's page and every flight has landed where the tabs say the deck
    /// heads. A deck that arrived on a page lays the grid down behind it.
    pub(super) fn check_arrival(self: Handle<Self>, app: &mut App) {
        let state = app.get(self);
        let at_rest = state.strip_grip.is_free()
            && state.strip_target.is_none()
            && state.overview_grip.is_free();
        let strip_target = state.followed.current_index() as f64;
        let on_a_page = !state.followed.overview_open;
        let arrived = at_rest
            && self.strip(app).value(app) == strip_target
            && self.every_flight_has_landed(app);
        if !arrived {
            return;
        }
        if on_a_page {
            self.lay_grid_down(app);
        }
        self.tabs(app).update(app, Tabs::arrived);
    }

    /// Takes every value out of the fingers' hands: a finger still down does nothing more, and
    /// a card it held springs back to its place.
    fn drop_every_grip(self: Handle<Self>, app: &mut App) {
        let state = app.get_mut(self);
        state.strip_grip.drop();
        state.overview_grip.drop();
        state.grid_grip.drop();
        state.held_page = None;
        self.drop_card_grips(app);
    }

    /// The view changed its size: what a finger held is let go, and the deck settles in the new
    /// size.
    fn view_resized(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        self.drop_every_grip(app);
        self.follow_tabs(app, StripFollows::Unseen);
    }

    /// Takes `layout` as the view's measure from now on; a view that changed its size lets go of
    /// what a finger held, after this frame.
    fn measure(self: Handle<Self>, app: &mut App, layout: DeckLayout) {
        let before = app.get_mut(self).layout.replace(layout);
        if before.is_some_and(|before| before != layout) {
            SchedulerBinding::add_post_frame_callback(
                app,
                FrameCallback::new(move |app, _| self.view_resized(app)),
                None,
            );
        }
    }

    /// Every page the deck draws, in the tabs' order: each tab's, and the home's past the last
    /// while no home is open.
    fn shown_tabs(self: Handle<Self>, app: &App) -> Vec<ShownTab> {
        let widget = self.widget(app);
        let (tabs, library) = (widget.tabs.read(app), widget.library.read(app));
        let mut shown: Vec<ShownTab> = tabs
            .tabs()
            .map(|tab| {
                let program = match &tab.content {
                    TabContent::Home => None,
                    TabContent::Program { id, .. } => Some(NamedProgram::of(id, library)),
                };
                ShownTab {
                    tab: Some(tab.id),
                    key: match tab.content {
                        TabContent::Home => FrameKey::Home,
                        TabContent::Program { .. } => FrameKey::Program(tab.id),
                    },
                    shown: PageShown::of(&tab.content, program.as_ref()),
                    program,
                }
            })
            .collect();
        if tabs.list().home().is_none() {
            shown.push(ShownTab {
                tab: None,
                key: FrameKey::Home,
                shown: PageShown::Home,
                program: None,
            });
        }
        shown
    }

    /// Builds the page of each of `tabs` that shows something it was not built for, and keeps no
    /// page of a tab that is gone.
    fn provide_pages(self: Handle<Self>, app: &mut App, tabs: &[ShownTab]) {
        let widget = self.widget(app);
        let (library, recordings) = (widget.library.clone(), widget.recordings.clone());
        let pages = &mut app.get_mut(self).pages;
        pages.retain(|key, _| tabs.iter().any(|tab| tab.key == *key));
        for tab in tabs {
            let built = pages
                .get(&tab.key)
                .is_some_and(|page| page.shown == tab.shown && page.program == tab.program);
            if built {
                continue;
            }
            let page = TabPage {
                shown: tab.shown.clone(),
                program: tab.program.clone(),
                library: library.clone(),
                recordings: recordings.clone(),
            };
            pages.insert(
                tab.key,
                BuiltPage {
                    shown: tab.shown.clone(),
                    program: tab.program.clone(),
                    widget: page.into_widget(),
                },
            );
        }
    }

    /// Where each of `tabs` is drawn now.
    fn placed<'a>(self: Handle<Self>, app: &App, tabs: &'a [ShownTab]) -> Vec<PlacedTab<'a>> {
        let layout = self.layout(app);
        let position = self.position(app);
        let cards = self.card_positions(app);
        let mut placements = layout.place_tabs(&cards, &position);
        placements.push(layout.place_page_past_the_last(
            cards.len(),
            &position,
            self.openness(app),
        ));
        tabs.iter()
            .zip(placements)
            .map(|(tab, placement)| PlacedTab { tab, placement })
            .collect()
    }

    /// `placed` in the order the tabs are painted in: the cards standing in the grid beneath the
    /// pages in flight, the current tab's over the others', and over them all a card on its way
    /// off the screen.
    fn in_paint_order<'a, 'b>(
        self: Handle<Self>,
        app: &App,
        placed: &'a [PlacedTab<'b>],
    ) -> Vec<&'a PlacedTab<'b>> {
        let state = app.get(self);
        let rank = |placed: &PlacedTab| {
            let Some((tab, card)) = placed
                .tab
                .tab
                .and_then(|tab| Some((tab, state.cards.get(&tab)?)))
            else {
                return 0;
            };
            let thrown = state.leaving.contains(&tab) || card.throw.value(app) != 0.0;
            let in_flight = card.flight.value(app) != 1.0
                || card.carry.iter().any(|axis| axis.value(app) != 0.0);
            match (thrown, in_flight, state.followed.current == Some(tab)) {
                (true, _, _) => 3,
                (false, true, true) => 2,
                (false, true, false) => 1,
                (false, false, _) => 0,
            }
        };
        let mut in_paint_order: Vec<&PlacedTab> = placed.iter().collect();
        in_paint_order.sort_by_key(|placed| rank(placed));
        in_paint_order
    }
}

/// What a page is told of the box it is laid out in, in the view `context` is in, laid out as
/// `layout`: the page's box, the system's bars and gesture zones at the top and the sides still
/// covering it, the bottom the bar's, and a keyboard covering only what of it reaches above the
/// bar.
fn page_media(app: &mut App, context: BuildContext, layout: &DeckLayout) -> MediaQueryData {
    let without_bottom =
        |insets: EdgeInsets| EdgeInsets::only(insets.left, insets.top, insets.right, 0.0);
    let padding = MediaQuery::padding_of(app, context);
    let view_padding = MediaQuery::view_padding_of(app, context);
    let system_gesture_insets = MediaQuery::system_gesture_insets_of(app, context);
    let insets = MediaQuery::view_insets_of(app, context);
    let keyboard_over_page = (insets.bottom - layout.bar_extent()).max(0.0);
    MediaQuery::of(app, context)
        .size(layout.page_box().size())
        .padding(without_bottom(padding))
        .view_padding(without_bottom(view_padding))
        .system_gesture_insets(without_bottom(system_gesture_insets))
        .view_insets(EdgeInsets::only(
            insets.left,
            insets.top,
            insets.right,
            keyboard_over_page,
        ))
}

impl TickerProviderStateMixin for DeckViewState {
    fn ticker_provider_data(self: Handle<Self>, app: &App) -> &TickerProviderStateMixinData {
        &app.get(self).ticker_provider
    }

    fn ticker_provider_data_mut(
        self: Handle<Self>,
        app: &mut App,
    ) -> &mut TickerProviderStateMixinData {
        &mut app.get_mut(self).ticker_provider
    }
}

impl TickerProviderObject for DeckViewState {
    fn create_ticker(self: Handle<Self>, app: &mut App, on_tick: TickerCallback) -> Ref<Ticker> {
        TickerProviderStateMixin::create_ticker(self, app, on_tick)
    }
}

impl State for DeckViewState {
    type Widget = DeckView;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let tabs = self.tabs(app);
        let followed = Followed::of(tabs.read(app));
        let motions = DeckMotions {
            strip: self.new_motion(app, followed.current_index() as f64),
            grid_scroll: self.new_motion(app, 0.0),
        };
        let tab_changes = app.observe(&tabs, move |app| self.tabs_changed_from_outside(app));
        let search = self.new_search_field(app);
        let state = app.get_mut(self);
        state.search = Some(search);
        state.motions = Some(motions);
        state.grid_stands = followed.overview_open;
        state.followed = followed;
        state.tab_changes = Some(tab_changes);
        self.provide_cards(app);
        self.hear_back(app);
    }

    fn activate(self: Handle<Self>, app: &mut App) {
        TickerProviderStateMixin::activate(self, app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).tab_changes = None;
        self.stop_hearing_back(app);
        self.dispose_search_field(app);
        let cards: Vec<TabId> = app.get(self).cards.keys().copied().collect();
        for tab in cards {
            self.forget_card(app, tab);
        }
        if let Some(motions) = app.get_mut(self).motions.take() {
            for motion in [motions.strip, motions.grid_scroll] {
                motion.dispose(app);
            }
        }
        TickerProviderStateMixin::dispose(self, app);
    }

    /// Reads the tabs and the library, so the deck rebuilds when they change. Every child of the
    /// stack is keyed, the toolbar coming and going and the tabs changing their painting order
    /// between the others. The stack clips nothing: it fills the deck's view, which bounds what is drawn in it, and a page off the
    /// strip, laid out past the view's edge, would otherwise have it clip at every frame.
    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let layout = DeckLayout::new(
            MediaQuery::size_of(app, context),
            MediaQuery::padding_of(app, context).top,
            beneath_the_bar(
                MediaQuery::view_padding_of(app, context).bottom,
                MediaQuery::system_gesture_insets_of(app, context).bottom,
            ),
        );
        self.measure(app, layout);
        let tabs = self.shown_tabs(app);
        self.provide_pages(app, &tabs);
        let placed = self.placed(app, &tabs);
        let page_media = page_media(app, context, &layout);
        let background = resolved(CupertinoColors::SYSTEM_GROUPED_BACKGROUND, app, context);
        let mut children =
            vec![DeckPart::Background.keyed(ColoredBox::new(background).into_widget())];
        children.extend(self.tab_frames(app, context, &placed, &page_media));
        children.push(DeckPart::TouchLayer.keyed(self.touch_layer(app, &placed)));
        let toolbar = self.overview_toolbar(app, context);
        children.extend(toolbar.map(|toolbar| DeckPart::Toolbar.keyed(toolbar)));
        children.push(DeckPart::Bar.keyed(self.bottom_bar(app, context, &placed)));
        let widget = self.widget(app);
        CupertinoAsked {
            library: widget.library.clone(),
            child: Stack::new()
                .fit(StackFit::Expand)
                .clip_behavior(Clip::None)
                .children(children)
                .into_widget(),
        }
        .into_widget()
    }
}
