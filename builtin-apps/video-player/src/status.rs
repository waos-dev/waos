//! The player's state, shown plainly over the picture, in Fluent's controls: a progress ring while
//! it loads or waits, with why; an informational bar on why the system paused it, with Play, and
//! a replay button at the end; and an error bar on why it failed, in the user's words with the
//! system's own beneath. A pause the user asked for, and a wait Apple advises showing nothing for
//! yet, show nothing here.

use waft_embedder::{
    PauseReason, PlayerActivity, PlayerError, PlayerErrorKind, PlayerState, PlayerStatus,
    WaitReason,
};
use waft_foundation::{App, Listener, Ref};
use waft_widgets::{
    BoxConstraints, Column, ConstrainedBox, EdgeInsetsGeometry, IntoWidget, MainAxisSize, Padding,
    Player, SizedBox, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, InfoBar, InfoBarSeverity, ProgressRing, TextBlockStyle};

use crate::icons::{self, icon};
use crate::text::{Tone, text};

/// The widest a bar over the picture is.
const BAR_WIDTH: f64 = 420.0;

/// What the state shows.
#[derive(Clone, Debug, PartialEq)]
enum Shown {
    Nothing,
    /// A spinner, with why it spins.
    Spinner(&'static str),
    /// The system paused it, for this reason, and Play plays on.
    PausedBySystem(&'static str),
    /// It reached its end: a button plays it again.
    Replay,
    /// It cannot play, why in the user's words, with the system's own.
    Failed {
        why: &'static str,
        details: String,
    },
}

/// What `state` shows.
fn shown(state: &PlayerState) -> Shown {
    match (&state.status, state.activity) {
        (PlayerStatus::Failed(error), _) => failed(error),
        (PlayerStatus::Loading, _) => Shown::Spinner("Loading"),
        (_, PlayerActivity::Waiting(reason)) => waiting(reason),
        (_, PlayerActivity::Paused(Some(PauseReason::End))) => Shown::Replay,
        (_, PlayerActivity::Paused(Some(reason))) => paused(reason),
        (_, PlayerActivity::Paused(None) | PlayerActivity::Playing) => Shown::Nothing,
    }
}

fn waiting(reason: Option<WaitReason>) -> Shown {
    match reason {
        Some(WaitReason::Data) | None => Shown::Spinner("Waiting for data"),
        Some(WaitReason::EvaluatingData) => Shown::Nothing,
        Some(WaitReason::HeldBySystem) => Shown::Spinner("Held back by the system"),
    }
}

fn paused(reason: PauseReason) -> Shown {
    match reason {
        PauseReason::Requested | PauseReason::End => Shown::Nothing,
        PauseReason::Interrupted => Shown::PausedBySystem("Paused: something else took the sound"),
        PauseReason::OutputRemoved => Shown::PausedBySystem("Paused: the sound's output went away"),
        PauseReason::Backgrounded => Shown::PausedBySystem("Paused in the background"),
    }
}

fn failed(error: &PlayerError) -> Shown {
    let why = match error.kind {
        PlayerErrorKind::Network => "The media could not be reached",
        PlayerErrorKind::Decode => "The media is damaged, or could not be decoded",
        PlayerErrorKind::Unsupported => "This format cannot be played here",
        PlayerErrorKind::Protected => "This media is protected, and cannot be played here",
        PlayerErrorKind::Refused => "The player may not reach this media",
        PlayerErrorKind::Other => "The media could not be played",
    };
    Shown::Failed {
        why,
        details: error.message.clone(),
    }
}

/// `player`'s state as [`shown`] has it, centred where the widget is put.
pub(crate) fn status_of(app: &App, player: &Ref<Player>) -> WidgetRef {
    match shown(player.state(app)) {
        Shown::Nothing => SizedBox::shrink().into_widget(),
        Shown::Spinner(why) => spinner(why),
        Shown::PausedBySystem(why) => paused_by_system(player, why),
        Shown::Replay => replay(player),
        Shown::Failed { why, details } => failure(why, &details),
    }
}

fn spinner(why: &str) -> WidgetRef {
    Column::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(12.0)
        .children([
            SizedBox::new()
                .width(48.0)
                .height(48.0)
                .child(ProgressRing::new())
                .into_widget(),
            text(why, TextBlockStyle::Body, Tone::Primary),
        ])
        .into_widget()
}

fn paused_by_system(player: &Ref<Player>, why: &str) -> WidgetRef {
    let play = Button::new(icon(icons::PLAY, 16.0), play(player)).style(ButtonStyle::Accent);
    let bar = InfoBar::new(true, |_, _| {})
        .severity(InfoBarSeverity::Informational)
        .message(why)
        .is_closable(false)
        .action_button(play);
    bounded(bar.into_widget())
}

fn replay(player: &Ref<Player>) -> WidgetRef {
    let face = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(4.0)
        .children([
            icon(icons::REPLAY, 32.0).into_widget(),
            text("Play again", TextBlockStyle::Body, Tone::Primary),
        ]);
    Button::new(
        Padding::new(EdgeInsetsGeometry::all(8.0)).child(face),
        play(player),
    )
    .into_widget()
}

fn failure(why: &str, details: &str) -> WidgetRef {
    let bar = InfoBar::new(true, |_, _| {})
        .severity(InfoBarSeverity::Error)
        .title(why)
        .message(details)
        .is_closable(false);
    bounded(bar.into_widget())
}

/// `bar`, no wider than [`BAR_WIDTH`].
fn bounded(bar: WidgetRef) -> WidgetRef {
    ConstrainedBox::new(BoxConstraints::new().max_width(BAR_WIDTH))
        .child(bar)
        .into_widget()
}

/// Plays `player`, from its start once it ended.
fn play(player: &Ref<Player>) -> Listener {
    let player = player.clone();
    Listener::new(move |app: &mut App| player.play(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(activity: PlayerActivity) -> PlayerState {
        PlayerState {
            status: PlayerStatus::Ready,
            activity,
            ..PlayerState::loading()
        }
    }

    #[test]
    fn loading_and_waiting_for_data_spin() {
        assert_eq!(shown(&PlayerState::loading()), Shown::Spinner("Loading"));
        let waiting = ready(PlayerActivity::Waiting(Some(WaitReason::Data)));
        assert_eq!(shown(&waiting), Shown::Spinner("Waiting for data"));
    }

    #[test]
    fn a_wait_still_judged_shows_nothing_yet_as_apple_advises() {
        let judging = ready(PlayerActivity::Waiting(Some(WaitReason::EvaluatingData)));
        assert_eq!(shown(&judging), Shown::Nothing);
    }

    #[test]
    fn the_end_offers_a_replay_and_the_systems_pauses_say_why() {
        assert_eq!(
            shown(&ready(PlayerActivity::Paused(Some(PauseReason::End)))),
            Shown::Replay
        );
        assert!(matches!(
            shown(&ready(PlayerActivity::Paused(Some(
                PauseReason::Interrupted
            )))),
            Shown::PausedBySystem(_)
        ));
        assert_eq!(
            shown(&ready(PlayerActivity::Paused(Some(PauseReason::Requested)))),
            Shown::Nothing
        );
        assert_eq!(shown(&ready(PlayerActivity::Playing)), Shown::Nothing);
    }

    #[test]
    fn a_failure_says_its_kind_with_the_systems_words() {
        let failed = PlayerState::failed(PlayerError {
            kind: PlayerErrorKind::Refused,
            message: "the program may not reach example.com".to_owned(),
        });
        assert_eq!(
            shown(&failed),
            Shown::Failed {
                why: "The player may not reach this media",
                details: "the program may not reach example.com".to_owned(),
            }
        );
    }
}
