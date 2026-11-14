//! The app's state, as every command and every widget reaches it: the library and where it is
//! kept, what the user chose, what the window shows, the refreshes under way, the pictures
//! fetched, and the navigator of the window's app, which its dialogs are shown over.

use waft::{App, Entity, GlobalKey};
use waft_winui::InfoBarSeverity;

use crate::browsing::{Browsing, Notice};
use crate::library::Library;
use crate::pictures::Pictures;
use crate::refreshing::Refreshing;
use crate::settings::Settings;
use crate::starting_feeds::starting_library;
use crate::store::Store;

#[derive(Clone, Debug)]
pub(crate) struct ReaderApp {
    pub(crate) library: Entity<Library>,
    pub(crate) settings: Entity<Settings>,
    pub(crate) browsing: Entity<Browsing>,
    pub(crate) refreshing: Entity<Refreshing>,
    pub(crate) pictures: Entity<Pictures>,
    pub(crate) navigator: GlobalKey,
    store: Store,
}

impl ReaderApp {
    /// The app over the library the program's `/data` keeps; at the first launch, and where what
    /// was kept cannot be read, over the feeds a first launch starts with.
    pub(crate) fn open(app: &mut App) -> ReaderApp {
        let store = Store::in_data();
        let library = match store.load() {
            Ok(Some(followed)) => Library::of(followed),
            Ok(None) => starting_library(),
            Err(error) => {
                eprintln!("the library kept is not read, and the app starts anew: {error}");
                starting_library()
            }
        };
        let settings = store.load_settings();
        let reader = ReaderApp {
            library: app.new_entity(|_cx| library),
            settings: app.new_entity(|_cx| settings),
            browsing: app.new_entity(|_cx| Browsing::default()),
            refreshing: app.new_entity(|_cx| Refreshing::default()),
            pictures: app.new_entity(|_cx| Pictures::default()),
            navigator: GlobalKey::labeled("reader"),
            store,
        };
        reader.keep_library(app);
        reader
    }

    /// Changes the library as `change` does, shows the change and keeps it.
    pub(crate) fn change_library<R>(
        &self,
        app: &mut App,
        change: impl FnOnce(&mut Library) -> R,
    ) -> R {
        let changed = self.library.update(app, |library, cx| {
            cx.notify();
            change(library)
        });
        self.keep_library(app);
        changed
    }

    /// Changes what the user chose as `change` does, shows the change and keeps it.
    pub(crate) fn change_settings(&self, app: &mut App, change: impl FnOnce(&mut Settings)) {
        let settings = self.settings.update(app, |settings, cx| {
            change(settings);
            cx.notify();
            *settings
        });
        if let Err(error) = self.store.keep_settings(settings) {
            eprintln!("the settings are not kept: {error}");
        }
    }

    /// Says `message` above the window's content, as `severity` says, until the user closes it.
    pub(crate) fn say(&self, app: &mut App, severity: InfoBarSeverity, message: impl Into<String>) {
        let notice = Notice {
            severity,
            message: message.into(),
        };
        self.browsing.update(app, |browsing, cx| {
            browsing.say(notice);
            cx.notify();
        });
    }

    /// Keeps what changed of the library since it was last kept.
    fn keep_library(&self, app: &mut App) {
        let changes = self
            .library
            .update(app, |library, _cx| library.take_changes());
        if let Err(error) = self.store.keep(self.library.read(app), &changes) {
            eprintln!("the library is not kept: {error}");
            self.library
                .update(app, |library, _cx| library.still_to_keep(changes));
        }
    }
}
