//! The system's Back on the deck, as methods of the deck's state, which hears it as Waft's binding
//! asks the app's observers to pop a route (`EmbedderClient::pop_route`, which Waft's Android host
//! calls for the back button and the back gesture). In this order: the overview, open or opening,
//! closes, as its Done does; the page of a program that goes back within itself sends the program
//! the press, which it handles; the page of any other program goes home, as the overview's plus
//! does, the program keeping its tab and running on; and the home goes back to its first page from
//! another of its own, as a navigator's back does. On the home's first page the press is left
//! unhandled, for the system's own: Waft's Android host hands it to Android, which sends WaOS to
//! the background.
//!
//! A keyboard up has the press before the deck: Android's input method takes Back to hide itself,
//! before the app's own window hears it, so the deck never sees a Back while a field's keyboard is
//! up.
//!
//! The invariant: Back moves the deck only through the calls its own controls make, so its
//! springs and its grip rules are theirs, and it asks a program to go back only while the program
//! says it handles Back.

use std::rc::Rc;

use waft::ui::ViewId;
use waft::{
    App, Handle, State, View, WidgetsBinding, WidgetsBindingObserverObject,
    WidgetsBindingObserverRef,
};

use super::DeckViewState;
use crate::state::deck::tabs::{TabContent, TabStage};
use crate::state::library::LibraryPage;

impl DeckViewState {
    /// From now on the deck hears the system's Back.
    pub(super) fn hear_back(self: Handle<Self>, app: &mut App) {
        let back: WidgetsBindingObserverRef = Rc::new(self);
        WidgetsBinding::instance(app).add_observer(app, Rc::clone(&back));
        app.get_mut(self).back = Some(back);
    }

    /// The deck hears the system's Back no more.
    pub(super) fn stop_hearing_back(self: Handle<Self>, app: &mut App) {
        if let Some(back) = app.get_mut(self).back.take() {
            WidgetsBinding::instance(app).remove_observer(app, &back);
        }
    }

    /// The system's Back: answers whether the deck handled it, as the module says.
    fn go_back(self: Handle<Self>, app: &mut App) -> bool {
        if self.tabs(app).read(app).list().overview_open() {
            self.close_overview(app);
            return true;
        }
        if self.send_back_to_the_current_program(app) {
            return true;
        }
        if self.on_a_programs_page(app) {
            self.go_home(app);
            return true;
        }
        self.go_back_on_the_home(app)
    }

    /// Sends the press to the program on the current page, where it runs and says it handles
    /// Back; answers whether it did.
    fn send_back_to_the_current_program(self: Handle<Self>, app: &App) -> bool {
        let tabs = self.tabs(app);
        let TabContent::Program {
            stage: TabStage::Running(shown),
            ..
        } = tabs.read(app).current_tab().content
        else {
            return false;
        };
        let shown = shown.read(app);
        if !shown.handles_back() {
            return false;
        }
        shown.pop_route();
        true
    }

    /// Whether the current page is a program's, starting, running or ended.
    fn on_a_programs_page(self: Handle<Self>, app: &App) -> bool {
        let tabs = self.tabs(app);
        matches!(
            tabs.read(app).current_tab().content,
            TabContent::Program { .. }
        )
    }

    /// The home's page: from a page of its own other than its first, the library goes back to
    /// the first; answers whether it did.
    fn go_back_on_the_home(self: Handle<Self>, app: &mut App) -> bool {
        let library = self.widget(app).library.clone();
        if *library.read(app).page() == LibraryPage::Home {
            return false;
        }
        library.update(app, |library, cx| library.show_page(LibraryPage::Home, cx));
        true
    }
}

impl WidgetsBindingObserverObject for DeckViewState {
    /// Back in the deck's own view, as Waft's `WidgetsApp` answers only for the view it is in.
    fn did_pop_route(self: Handle<Self>, app: &mut App, view: ViewId) -> bool {
        if View::maybe_id_of(app, self.context(app)) != Some(view) {
            return false;
        }
        self.go_back(app)
    }
}
