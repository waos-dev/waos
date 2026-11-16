//! The window's page: what plays, under its state and its controls, with the way back to the
//! library; or, while nothing does, the library (`library_screen`).

use std::fmt;
use std::rc::Rc;

use waft_foundation::{App, Entity, Listenable, Listener, Ref, ValueKey};
use waft_widgets::{
    BuildContext, Center, IntoWidget, KeyedSubtree, ListenableBuilder, Player, Stack, StackFit,
    StatelessWidget, WidgetRef,
};
use waft_winui::{Brush, Page, TextBlockStyle, ThemeResources};

use crate::controls::Controls;
use crate::keys::BackOnEscape;
use crate::library::Library;
use crate::library_screen::LibraryScreen;
use crate::playback::{self, Playback, Session};
use crate::status;
use crate::text::{Tone, text};
use crate::video;
use crate::window::{APP_NAME, PlayerWindow, title_for};

/// What the window shows of `playback`, and of `library` while nothing plays.
pub(crate) struct Screen {
    playback: Entity<Playback>,
    library: Entity<Library>,
}

impl Screen {
    pub(crate) fn new(playback: Entity<Playback>, library: Entity<Library>) -> Screen {
        Screen { playback, library }
    }
}

impl fmt::Debug for Screen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Screen").finish_non_exhaustive()
    }
}

impl StatelessWidget for Screen {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        if app.platform().players().is_none() {
            let none = Center::new().child(text(
                "This system has no media players",
                TextBlockStyle::BodyLarge,
                Tone::Primary,
            ));
            let page = on_page(app, context, none.into_widget());
            return PlayerWindow::new(APP_NAME.to_owned(), page).into_widget();
        }
        match self.playback.read(app).session().cloned() {
            Some(session) => playing(&self.playback, session),
            None => {
                let library = LibraryScreen::new(self.library.clone(), self.playback.clone());
                let page = on_page(app, context, library.into_widget());
                PlayerWindow::new(APP_NAME.to_owned(), page).into_widget()
            }
        }
    }
}

/// `content` on Fluent's background, which reaches under a phone's bars while the content stays
/// clear of them and of the keyboard.
fn on_page(app: &mut App, context: BuildContext, content: WidgetRef) -> WidgetRef {
    let background = ThemeResources::of(app, context).solid_background_fill_color_base;
    Page::new()
        .background(Brush::Solid(background))
        .content(content)
        .into_widget()
}

/// `session`'s picture, its state and its controls, the way back to the library among them, in a
/// window titled for it; built anew for each session. The picture fills the view, under a phone's
/// bars too, as a phone's own player fills the screen, while the controls keep clear of the bars.
fn playing(playback: &Entity<Playback>, session: Session) -> WidgetRef {
    let player = session.player;
    let stating = player.clone();
    let state = ListenableBuilder::new(heard(&player), move |app, _context, _child| {
        status::status_of(app, &stating)
    });
    let picture = Stack::new().fit(StackFit::Expand).children([
        video::letterboxed(&player),
        Center::new().child(state).into_widget(),
    ]);
    let back = {
        let playback = playback.clone();
        Listener::new(move |app: &mut App| playback::close(&playback, app))
    };
    let page = Controls::new(player.clone(), picture.into_widget(), back).into_widget();
    let name = session.source.name();
    let titled = ListenableBuilder::new(heard(&player), move |app, _context, _child| {
        let title = title_for(&name, player.metadata(app).title.as_deref());
        PlayerWindow::new(title, page.clone()).into_widget()
    });
    KeyedSubtree::new(BackOnEscape::new(playback.clone(), titled.into_widget()))
        .key(ValueKey::new(session.number).into())
        .into_widget()
}

/// `player`, as what a builder rebuilds at each change of.
fn heard(player: &Ref<Player>) -> Rc<dyn Listenable> {
    Rc::new(player.clone())
}
