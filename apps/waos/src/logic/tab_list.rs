//! The deck's tabs as a list: what each tab is, the home or a program, their order, the tab the
//! deck is on or heading to, whether its overview is open, and which pages are in sight meanwhile.
//! Plain values: it starts no program and knows nothing of how the deck moves.
//!
//! The rules. A tab is the home until a program is opened, and then that tab is the program's; a
//! program that has a tab is brought into sight instead of opened again. There is one home at
//! most: going home goes to it where it exists and makes it at the end where it does not, so the
//! home is the last tab whenever it is open. There is always a tab: closing the last one leaves a
//! home, and the home alone cannot be closed.
//!
//! Where the deck is heading, a tab's page or the overview, changes at once; that it got there is
//! said by whoever moves it ([`TabList::arrived`]). A page is in sight while the deck heads to it
//! or rests on it, and the page the deck last rested on stays in sight until the deck arrives
//! somewhere else. A page has the deck's attention only while the deck rests on it.
//!
//! The invariant: `current` always names a tab of the list, the list is never empty, and at most
//! one tab is the home, the last: a tab is only ever added at the end, and a program's only where
//! no home is open.

use waos_package::ProgramId;

/// One tab, for as long as it is open: the same tab while the home in it becomes a program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TabId(u64);

/// What a tab is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TabKind {
    /// The home: the library's programs, to open one from.
    Home,
    /// The program opened in it.
    Program(ProgramId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListedTab {
    pub(crate) id: TabId,
    pub(crate) kind: TabKind,
}

/// Where a program opened went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// It had a tab, which the deck heads to.
    BroughtIntoSight(TabId),
    /// The home's tab is its tab now.
    InTheHomesTab(TabId),
    /// No home was open: a tab was made for it at the end.
    InANewTab(TabId),
}

impl Opened {
    pub(crate) fn tab(self) -> TabId {
        match self {
            Opened::BroughtIntoSight(tab) | Opened::InTheHomesTab(tab) | Opened::InANewTab(tab) => {
                tab
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TabList {
    tabs: Vec<ListedTab>,
    /// The tab the deck is on, or heading to.
    current: TabId,
    /// Whether the deck shows, or is heading to, its overview of every tab.
    overview_open: bool,
    /// Whether the deck rests where it was heading.
    arrived: bool,
    /// The tab whose page the deck rested on before it headed elsewhere, in sight until it
    /// arrives.
    left_behind: Option<TabId>,
    next_id: u64,
}

impl TabList {
    /// One tab, the home, which the deck rests on.
    pub(crate) fn new() -> TabList {
        let home = TabId(0);
        TabList {
            tabs: vec![ListedTab {
                id: home,
                kind: TabKind::Home,
            }],
            current: home,
            overview_open: false,
            arrived: true,
            left_behind: None,
            next_id: 1,
        }
    }

    /// Each tab, in order.
    pub(crate) fn tabs(&self) -> &[ListedTab] {
        &self.tabs
    }

    pub(crate) fn current(&self) -> TabId {
        self.current
    }

    pub(crate) fn index_of(&self, tab: TabId) -> Option<usize> {
        self.tabs.iter().position(|listed| listed.id == tab)
    }

    pub(crate) fn kind_of(&self, tab: TabId) -> Option<&TabKind> {
        self.tabs
            .iter()
            .find(|listed| listed.id == tab)
            .map(|listed| &listed.kind)
    }

    pub(crate) fn overview_open(&self) -> bool {
        self.overview_open
    }

    /// Whether the deck rests where it was heading.
    pub(crate) fn has_arrived(&self) -> bool {
        self.arrived
    }

    /// The home's tab, where one is open.
    pub(crate) fn home(&self) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|listed| listed.kind == TabKind::Home)
            .map(|listed| listed.id)
    }

    /// The tab of the program `program`, where it has one.
    pub(crate) fn tab_of(&self, program: &ProgramId) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|listed| matches!(&listed.kind, TabKind::Program(id) if id == program))
            .map(|listed| listed.id)
    }

    /// Whether the page of `tab` is in sight: the deck heads to it or rests on it, or rested on it
    /// last and has not arrived elsewhere yet.
    pub(crate) fn in_sight(&self, tab: TabId) -> bool {
        (!self.overview_open && self.current == tab) || self.left_behind == Some(tab)
    }

    /// Whether the deck rests on the page of `tab`.
    pub(crate) fn rests_on(&self, tab: TabId) -> bool {
        self.arrived && !self.overview_open && self.current == tab
    }

    /// The program `program` is opened: the deck heads to the page of its tab, the one it has, or
    /// the home's, which becomes its own, or a new one at the end where no home is open.
    pub(crate) fn open_program(&mut self, program: &ProgramId) -> Opened {
        let opened = if let Some(tab) = self.tab_of(program) {
            Opened::BroughtIntoSight(tab)
        } else if let Some(home) = self.home() {
            self.set_kind(home, TabKind::Program(program.clone()));
            Opened::InTheHomesTab(home)
        } else {
            Opened::InANewTab(self.add(TabKind::Program(program.clone())))
        };
        self.head_for(opened.tab(), false);
        opened
    }

    /// The program of `tab` could not start: the tab is the home again, or goes where a home is
    /// open already.
    pub(crate) fn program_did_not_start(&mut self, tab: TabId) {
        if !matches!(self.kind_of(tab), Some(TabKind::Program(_))) {
            return;
        }
        if self.home().is_some() {
            self.close(tab);
        } else {
            self.set_kind(tab, TabKind::Home);
        }
    }

    /// The deck heads to the page of `tab`; nothing where it is not open.
    pub(crate) fn show_page(&mut self, tab: TabId) {
        if self.index_of(tab).is_some() {
            self.head_for(tab, false);
        }
    }

    /// The deck heads to its overview, or back to the page of the current tab.
    pub(crate) fn set_overview_open(&mut self, open: bool) {
        self.head_for(self.current, open);
    }

    /// The deck heads to the home's page, where the home is, made at the end where none is open;
    /// answers its tab.
    pub(crate) fn go_home(&mut self) -> TabId {
        let home = self.home().unwrap_or_else(|| self.add(TabKind::Home));
        self.head_for(home, false);
        home
    }

    /// Closes `tab`, and answers what it was; none where it is not open, or is the home alone.
    /// The deck heads to the tab that takes its place where it was the current one, and the last
    /// tab closed leaves a home.
    pub(crate) fn close(&mut self, tab: TabId) -> Option<TabKind> {
        let index = self.index_of(tab)?;
        if self.tabs.len() == 1 && self.tabs[0].kind == TabKind::Home {
            return None;
        }
        let closed = self.tabs.remove(index).kind;
        if self.tabs.is_empty() {
            self.add(TabKind::Home);
        }
        if self.left_behind == Some(tab) {
            self.left_behind = None;
        }
        if self.current == tab {
            let takes_its_place = self.tabs[index.min(self.tabs.len() - 1)].id;
            self.current = takes_its_place;
            self.arrived = false;
        }
        Some(closed)
    }

    /// The deck rests where it was heading: only what it rests on is in sight from now on.
    pub(crate) fn arrived(&mut self) {
        self.arrived = true;
        self.left_behind = None;
    }

    /// The deck heads to the page of `tab`, or to the overview with `tab` current. A page it
    /// rested on stays in sight until it arrives.
    fn head_for(&mut self, tab: TabId, overview_open: bool) {
        if (self.current, self.overview_open) == (tab, overview_open) {
            return;
        }
        if self.arrived && !self.overview_open {
            self.left_behind = Some(self.current);
        }
        self.current = tab;
        self.overview_open = overview_open;
        self.arrived = false;
    }

    fn add(&mut self, kind: TabKind) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        self.tabs.push(ListedTab { id, kind });
        id
    }

    fn set_kind(&mut self, tab: TabId, kind: TabKind) {
        if let Some(listed) = self.tabs.iter_mut().find(|listed| listed.id == tab) {
            listed.kind = kind;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(name: &str) -> ProgramId {
        format!("dev.example.{name}").parse().unwrap()
    }

    fn kinds(list: &TabList) -> Vec<TabKind> {
        list.tabs().iter().map(|tab| tab.kind.clone()).collect()
    }

    /// A list with `names` opened one after another, each from a home gone to first, the deck
    /// resting on the last one's page.
    fn with_programs(names: &[&str]) -> TabList {
        let mut list = TabList::new();
        for name in names {
            list.go_home();
            list.open_program(&program(name));
        }
        list.arrived();
        list
    }

    #[test]
    fn the_deck_starts_resting_on_the_home() {
        let list = TabList::new();
        assert_eq!(kinds(&list), [TabKind::Home]);
        assert_eq!(Some(list.current()), list.home());
        assert!(list.rests_on(list.current()) && !list.overview_open());
    }

    #[test]
    fn a_program_opened_takes_the_homes_tab() {
        let mut list = TabList::new();
        let home = list.current();
        let opened = list.open_program(&program("a"));
        assert_eq!(opened, Opened::InTheHomesTab(home));
        assert_eq!(kinds(&list), [TabKind::Program(program("a"))]);
        assert_eq!(list.home(), None);
        assert!(list.rests_on(home), "the deck never left the tab");
    }

    #[test]
    fn a_program_opened_with_no_home_open_gets_a_tab_at_the_end() {
        let mut list = with_programs(&["a"]);
        let opened = list.open_program(&program("b"));
        assert!(matches!(opened, Opened::InANewTab(_)));
        assert_eq!(
            kinds(&list),
            [
                TabKind::Program(program("a")),
                TabKind::Program(program("b"))
            ]
        );
        assert_eq!(list.current(), opened.tab());
    }

    #[test]
    fn a_program_that_has_a_tab_is_brought_into_sight() {
        let mut list = with_programs(&["a", "b"]);
        let a = list.tab_of(&program("a")).unwrap();
        list.set_overview_open(true);
        let opened = list.open_program(&program("a"));
        assert_eq!(opened, Opened::BroughtIntoSight(a));
        assert_eq!(list.tabs().len(), 2, "no second tab for it");
        assert_eq!(list.current(), a);
        assert!(!list.overview_open(), "its page, not the overview");
    }

    #[test]
    fn going_home_goes_to_the_one_home_or_makes_it_at_the_end() {
        let mut list = with_programs(&["a"]);
        let made = list.go_home();
        assert_eq!(
            kinds(&list),
            [TabKind::Program(program("a")), TabKind::Home]
        );
        list.open_program(&program("b"));
        let again = list.go_home();
        assert_ne!(again, made, "the first home became b's tab");
        assert_eq!(list.go_home(), again, "there is one home at most");
        assert_eq!(list.tabs().len(), 3);
        assert_eq!(list.current(), again);
    }

    #[test]
    fn the_home_is_the_last_tab_whenever_it_is_open() {
        let mut list = with_programs(&["a", "b"]);
        list.go_home();
        list.show_page(list.tab_of(&program("a")).unwrap());
        list.open_program(&program("c"));
        list.go_home();
        list.close(list.tab_of(&program("b")).unwrap());
        assert_eq!(
            kinds(&list),
            [
                TabKind::Program(program("a")),
                TabKind::Program(program("c")),
                TabKind::Home
            ]
        );
    }

    #[test]
    fn closing_the_current_tab_heads_to_the_one_that_takes_its_place() {
        let mut list = with_programs(&["a", "b", "c"]);
        let (a, b, c) = (
            list.tab_of(&program("a")).unwrap(),
            list.tab_of(&program("b")).unwrap(),
            list.tab_of(&program("c")).unwrap(),
        );
        list.show_page(b);
        assert_eq!(list.close(b), Some(TabKind::Program(program("b"))));
        assert_eq!(list.current(), c, "the tab after it");
        assert_eq!(list.close(c), Some(TabKind::Program(program("c"))));
        assert_eq!(list.current(), a, "the tab before it, at the end");
    }

    #[test]
    fn closing_another_tab_leaves_the_deck_where_it_is() {
        let mut list = with_programs(&["a", "b"]);
        let (a, b) = (
            list.tab_of(&program("a")).unwrap(),
            list.tab_of(&program("b")).unwrap(),
        );
        assert!(list.rests_on(b));
        list.close(a);
        assert!(list.rests_on(b));
    }

    #[test]
    fn the_last_tab_closed_leaves_a_home_and_the_home_alone_stays() {
        let mut list = with_programs(&["a"]);
        let a = list.current();
        assert_eq!(list.close(a), Some(TabKind::Program(program("a"))));
        assert_eq!(kinds(&list), [TabKind::Home]);
        let home = list.current();
        assert_eq!(Some(home), list.home());
        assert_eq!(list.close(home), None);
        assert_eq!(kinds(&list), [TabKind::Home]);
    }

    #[test]
    fn a_home_among_other_tabs_closes() {
        let mut list = with_programs(&["a"]);
        let home = list.go_home();
        assert_eq!(list.close(home), Some(TabKind::Home));
        assert_eq!(kinds(&list), [TabKind::Program(program("a"))]);
    }

    #[test]
    fn a_program_that_did_not_start_gives_the_tab_back_to_the_home() {
        let mut list = TabList::new();
        let tab = list.open_program(&program("a")).tab();
        list.program_did_not_start(tab);
        assert_eq!(kinds(&list), [TabKind::Home]);
        assert_eq!(list.home(), Some(tab));
    }

    #[test]
    fn a_program_that_did_not_start_closes_its_tab_where_a_home_is_open() {
        let mut list = TabList::new();
        let tab = list.open_program(&program("a")).tab();
        let home = list.go_home();
        list.program_did_not_start(tab);
        assert_eq!(kinds(&list), [TabKind::Home]);
        assert_eq!(list.current(), home);
    }

    #[test]
    fn the_page_left_stays_in_sight_until_the_deck_arrives() {
        let mut list = with_programs(&["a", "b"]);
        let (a, b) = (
            list.tab_of(&program("a")).unwrap(),
            list.tab_of(&program("b")).unwrap(),
        );
        list.show_page(a);
        assert!(list.in_sight(a), "the page headed to");
        assert!(list.in_sight(b), "the page left, until the deck arrives");
        assert!(!list.rests_on(a) && !list.rests_on(b));
        list.arrived();
        assert!(list.rests_on(a));
        assert!(!list.in_sight(b));
    }

    #[test]
    fn nothing_is_in_sight_once_the_deck_rests_in_the_overview() {
        let mut list = with_programs(&["a"]);
        let a = list.current();
        list.set_overview_open(true);
        assert!(list.in_sight(a) && !list.rests_on(a));
        list.arrived();
        assert!(!list.in_sight(a));
        list.set_overview_open(false);
        assert!(list.in_sight(a) && !list.rests_on(a));
        list.arrived();
        assert!(list.rests_on(a));
    }

    #[test]
    fn heading_back_before_arriving_keeps_the_page_in_sight_but_not_rested_on() {
        let mut list = with_programs(&["a"]);
        let a = list.current();
        list.set_overview_open(true);
        list.set_overview_open(false);
        assert!(list.in_sight(a));
        assert!(!list.rests_on(a), "until the deck says it arrived");
        list.arrived();
        assert!(list.rests_on(a));
    }

    #[test]
    fn a_tab_closed_is_in_sight_no_more() {
        let mut list = with_programs(&["a", "b"]);
        let (a, b) = (
            list.tab_of(&program("a")).unwrap(),
            list.tab_of(&program("b")).unwrap(),
        );
        list.show_page(a);
        list.close(b);
        assert!(!list.in_sight(b));
        assert_eq!(list.index_of(b), None);
    }
}
