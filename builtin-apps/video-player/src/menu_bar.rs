//! The menu bar: the app's menu, File, Edit, Playback, View and Window, each item following what
//! plays. It is built again only when what it shows changes, since each new menu bar replaces the
//! one the user may have open, and a player's state changes many times a second while it loads.

use std::fmt;
use std::rc::Rc;

use waft_embedder::{CheckState, MenuItemCheck};
use waft_foundation::{App, Entity, Handle, Listenable, Listener};
use waft_services::LogicalKeyboardKey;
use waft_widgets::{
    BuildContext, GlobalKey, IntoWidget, KeyRef, PlainPlatformMenuItem, PlatformMenu,
    PlatformMenuBar, PlatformMenuItemGroup, PlatformMenuItemRef, PlatformProvidedMenuItem,
    PlatformProvidedMenuItemType, Player, SingleActivator, State, StateData, StatefulWidget,
    WidgetRef,
};

use crate::commands::{self, PlaybackCommand, Step};
use crate::library::Library;
use crate::opening;
use crate::playback::{self, Playback};
use crate::source::Source;
use crate::tracks::{self, TrackChoice};
use crate::window::APP_NAME;

/// `child` under the app's menu bar.
pub(crate) struct PlayerMenuBar {
    playback: Entity<Playback>,
    /// The library a folder is added to.
    library: Entity<Library>,
    /// The navigator the Open Location dialog is shown on.
    navigator: GlobalKey,
    child: WidgetRef,
}

impl PlayerMenuBar {
    pub(crate) fn new(
        playback: Entity<Playback>,
        library: Entity<Library>,
        navigator: GlobalKey,
        child: WidgetRef,
    ) -> PlayerMenuBar {
        PlayerMenuBar {
            playback,
            library,
            navigator,
            child,
        }
    }
}

impl fmt::Debug for PlayerMenuBar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlayerMenuBar").finish_non_exhaustive()
    }
}

impl StatefulWidget for PlayerMenuBar {
    type State = PlayerMenuBarState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> PlayerMenuBarState {
        PlayerMenuBarState {
            state: StateData::new(),
            heard: None,
            shown: None,
            menus: Vec::new(),
        }
    }
}

pub(crate) struct PlayerMenuBarState {
    state: StateData<PlayerMenuBar>,
    /// The player whose changes it hears: the one playing when it was built last.
    heard: Option<Handle<Player>>,
    /// What the menus show, as they were built last.
    shown: Option<MenuFacts>,
    menus: Vec<PlatformMenuItemRef>,
}

impl PlayerMenuBarState {
    fn listener(self: Handle<Self>) -> Listener {
        Listener::handle_method(self, Self::player_changed)
    }

    /// Hears `player` from now on, in place of the one heard before.
    fn hear(self: Handle<Self>, app: &mut App, player: Option<Handle<Player>>) {
        let heard = app.get(self).heard;
        if heard == player {
            return;
        }
        if let Some(heard) = heard {
            listenable(heard).remove_listener(app, &self.listener());
        }
        if let Some(player) = player {
            listenable(player).add_listener(app, self.listener());
        }
        app.get_mut(self).heard = player;
    }

    /// The player changed: the menus are built again if what they show did.
    fn player_changed(self: Handle<Self>, app: &mut App) {
        let facts = MenuFacts::of(app, &self.widget(app).playback.clone());
        if app.get(self).shown.as_ref() != Some(&facts) {
            self.set_state(app, |_| {});
        }
    }
}

impl State for PlayerMenuBarState {
    type Widget = PlayerMenuBar;
    waft_widgets::state_accessors!();

    fn dispose(self: Handle<Self>, app: &mut App) {
        self.hear(app, None);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let widget = self.widget(app);
        let (playback, library, navigator, child) = (
            widget.playback.clone(),
            widget.library.clone(),
            widget.navigator.clone(),
            widget.child.clone(),
        );
        let player = playback::current_player(&playback, app);
        self.hear(app, player);
        let facts = MenuFacts::of(app, &playback);
        if app.get(self).shown.as_ref() != Some(&facts) {
            let menus = menus(&facts, &playback, &library, &navigator);
            let state = app.get_mut(self);
            state.menus = menus;
            state.shown = Some(facts);
        }
        PlatformMenuBar::new(app.get(self).menus.clone())
            .child(child)
            .into_widget()
    }
}

fn listenable(player: Handle<Player>) -> Rc<dyn Listenable> {
    Rc::new(player)
}

/// What the menus show: the recent sources, and what plays.
#[derive(Clone, Debug, PartialEq)]
struct MenuFacts {
    recents: Vec<Source>,
    playing: Option<PlayingFacts>,
}

/// What the menus show of what plays.
#[derive(Clone, Debug, PartialEq)]
struct PlayingFacts {
    paused: bool,
    muted: bool,
    faster: bool,
    slower: bool,
    subtitles: Vec<TrackChoice>,
    audio: Vec<TrackChoice>,
}

impl MenuFacts {
    fn of(app: &App, playback: &Entity<Playback>) -> MenuFacts {
        let playback = playback.read(app);
        let recents = playback.recents().list().iter();
        MenuFacts {
            recents: recents.map(|recent| recent.source.clone()).collect(),
            playing: playback
                .session()
                .map(|session| PlayingFacts::of(session.player.state(app))),
        }
    }
}

impl PlayingFacts {
    fn of(state: &waft_embedder::PlayerState) -> PlayingFacts {
        PlayingFacts {
            paused: commands::is_paused(state),
            muted: state.muted,
            faster: commands::stepped_rate(state.rate, Step::Faster).is_some(),
            slower: commands::stepped_rate(state.rate, Step::Slower).is_some(),
            subtitles: tracks::subtitle_choices(state),
            audio: tracks::audio_choices(state),
        }
    }
}

/// The whole menu bar, as `facts` has it.
fn menus(
    facts: &MenuFacts,
    playback: &Entity<Playback>,
    library: &Entity<Library>,
    navigator: &GlobalKey,
) -> Vec<PlatformMenuItemRef> {
    vec![
        app_menu(),
        file_menu(facts, playback, library, navigator),
        edit_menu(),
        playback_menu(facts.playing.as_ref(), playback),
        view_menu(facts.playing.as_ref(), playback),
        window_menu(),
    ]
}

/// About, Hide, Hide Others, Show All and Quit, the platform's own.
fn app_menu() -> PlatformMenuItemRef {
    use PlatformProvidedMenuItemType as Provided;
    menu(
        APP_NAME,
        vec![
            provided(Provided::About),
            group(vec![
                provided(Provided::Hide),
                provided(Provided::HideOtherApplications),
                provided(Provided::ShowAllApplications),
            ]),
            provided(Provided::Quit),
        ],
    )
}

/// Open…, Open Location…, Open Recent, Add Folder…, and Close, which goes back to the library.
fn file_menu(
    facts: &MenuFacts,
    playback: &Entity<Playback>,
    library: &Entity<Library>,
    navigator: &GlobalKey,
) -> PlatformMenuItemRef {
    let opening = {
        let playback = playback.clone();
        item("Open…")
            .shortcut(Rc::new(command(LogicalKeyboardKey::KEY_O)))
            .on_selected(Rc::new(move |app| opening::open_file(&playback, app)))
    };
    let location = {
        let (playback, navigator) = (playback.clone(), navigator.clone());
        item("Open Location…")
            .shortcut(Rc::new(command(LogicalKeyboardKey::KEY_L)))
            .on_selected(Rc::new(move |app| {
                if let Some(context) = navigator.current_context(app) {
                    opening::open_location(&playback, app, context);
                }
            }))
    };
    let folder = {
        let library = library.clone();
        item("Add Folder…")
            .shortcut(Rc::new(command(LogicalKeyboardKey::KEY_O).shift(true)))
            .on_selected(Rc::new(move |app| opening::add_folder(&library, app)))
    };
    let mut close = item("Close").shortcut(Rc::new(command(LogicalKeyboardKey::KEY_W)));
    if facts.playing.is_some() {
        let playback = playback.clone();
        close = close.on_selected(Rc::new(move |app| playback::close(&playback, app)));
    }
    menu(
        "File",
        vec![
            group(vec![
                Rc::new(opening),
                Rc::new(location),
                recent_menu(&facts.recents, playback),
            ]),
            group(vec![Rc::new(folder)]),
            Rc::new(close),
        ],
    )
}

/// Each source opened lately, which opens it again, and Clear Menu.
fn recent_menu(recents: &[Source], playback: &Entity<Playback>) -> PlatformMenuItemRef {
    let reopen = recents.iter().map(|source| {
        let (playback, source) = (playback.clone(), source.clone());
        let item = item(&source.name()).on_selected(Rc::new(move |app| {
            playback::open(&playback, app, source.clone());
        }));
        Rc::new(item) as PlatformMenuItemRef
    });
    let mut clear = item("Clear Menu");
    if !recents.is_empty() {
        let playback = playback.clone();
        clear = clear.on_selected(Rc::new(move |app| playback::clear_recents(&playback, app)));
    }
    let reopen: Vec<_> = reopen.collect();
    let mut items = Vec::new();
    if !reopen.is_empty() {
        items.push(group(reopen));
    }
    items.push(Rc::new(clear));
    menu("Open Recent", items)
}

/// The platform's Undo, Redo, Cut, Copy, Paste and Select All, which reach the Open Location
/// dialog's field.
fn edit_menu() -> PlatformMenuItemRef {
    use PlatformProvidedMenuItemType as Provided;
    menu(
        "Edit",
        vec![
            group(vec![provided(Provided::Undo), provided(Provided::Redo)]),
            provided(Provided::Cut),
            provided(Provided::Copy),
            provided(Provided::Paste),
            provided(Provided::SelectAll),
        ],
    )
}

/// The Playback menu's commands, each with its key, while something plays.
fn playback_menu(
    playing: Option<&PlayingFacts>,
    playback: &Entity<Playback>,
) -> PlatformMenuItemRef {
    let item_of =
        |command| Rc::new(playback_item(command, playing, playback)) as PlatformMenuItemRef;
    use PlaybackCommand as Command;
    menu(
        "Playback",
        vec![
            group(vec![item_of(Command::PlayPause)]),
            group(vec![item_of(Command::Forward), item_of(Command::Back)]),
            group(vec![item_of(Command::Faster), item_of(Command::Slower)]),
            item_of(Command::Mute),
        ],
    )
}

/// The item of `command`, worded and checked as what plays has it, and chosen only where it
/// does something.
fn playback_item(
    command: PlaybackCommand,
    playing: Option<&PlayingFacts>,
    playback: &Entity<Playback>,
) -> PlainPlatformMenuItem {
    let label = match command {
        PlaybackCommand::PlayPause if playing.is_none_or(|playing| playing.paused) => "Play",
        PlaybackCommand::PlayPause => "Pause",
        PlaybackCommand::Forward => "Forward 10 Seconds",
        PlaybackCommand::Back => "Back 10 Seconds",
        PlaybackCommand::Faster => "Faster",
        PlaybackCommand::Slower => "Slower",
        PlaybackCommand::Mute => "Mute",
    };
    let mut item = item(label).shortcut(Rc::new(command.key()));
    if command == PlaybackCommand::Mute {
        let muted = playing.is_some_and(|playing| playing.muted);
        item = item.check(MenuItemCheck::Checkbox(if muted {
            CheckState::On
        } else {
            CheckState::Off
        }));
    }
    let does_something = playing.is_some_and(|playing| match command {
        PlaybackCommand::Faster => playing.faster,
        PlaybackCommand::Slower => playing.slower,
        _ => true,
    });
    if does_something {
        let playback = playback.clone();
        item = item.on_selected(Rc::new(move |app| {
            if let Some(player) = playback::current_player(&playback, app) {
                command.run(app, player);
            }
        }));
    }
    item
}

/// The platform's Enter Full Screen, then the subtitles and the audio to choose among.
fn view_menu(playing: Option<&PlayingFacts>, playback: &Entity<Playback>) -> PlatformMenuItemRef {
    let none = Vec::new();
    let (subtitles, audio) = playing.map_or((&none, &none), |playing| {
        (&playing.subtitles, &playing.audio)
    });
    menu(
        "View",
        vec![
            group(vec![provided(
                PlatformProvidedMenuItemType::ToggleFullScreen,
            )]),
            track_menu("Subtitles", subtitles, playback),
            track_menu("Audio", audio, playback),
        ],
    )
}

/// A submenu of `choices`, the chosen one marked; disabled while there are none.
fn track_menu(
    label: &str,
    choices: &[TrackChoice],
    playback: &Entity<Playback>,
) -> PlatformMenuItemRef {
    let items = choices.iter().map(|choice| {
        let (playback, pick) = (playback.clone(), choice.pick.clone());
        let item = item(&choice.label)
            .check(MenuItemCheck::Radio(choice.chosen))
            .on_selected(Rc::new(move |app| {
                if let Some(player) = playback::current_player(&playback, app) {
                    tracks::pick_track(app, player, &pick);
                }
            }));
        Rc::new(item) as PlatformMenuItemRef
    });
    menu(label, items.collect())
}

/// The platform's Minimize, Zoom and Bring All to Front.
fn window_menu() -> PlatformMenuItemRef {
    use PlatformProvidedMenuItemType as Provided;
    menu(
        "Window",
        vec![
            group(vec![
                provided(Provided::MinimizeWindow),
                provided(Provided::ZoomWindow),
            ]),
            provided(Provided::ArrangeWindowsInFront),
        ],
    )
}

fn menu(label: &str, items: Vec<PlatformMenuItemRef>) -> PlatformMenuItemRef {
    Rc::new(PlatformMenu::new(label, items))
}

/// Items between separators: at least one, as Waft's groups have it.
fn group(members: Vec<PlatformMenuItemRef>) -> PlatformMenuItemRef {
    Rc::new(PlatformMenuItemGroup::new(members))
}

fn item(label: &str) -> PlainPlatformMenuItem {
    PlainPlatformMenuItem::new(label)
}

fn provided(kind: PlatformProvidedMenuItemType) -> PlatformMenuItemRef {
    Rc::new(PlatformProvidedMenuItem::new(kind))
}

/// `key` with ⌘.
fn command(key: LogicalKeyboardKey) -> SingleActivator {
    SingleActivator::new(key).meta(true)
}
