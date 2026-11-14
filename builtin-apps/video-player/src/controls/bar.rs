//! The bar of controls along the bottom of the picture: back ten seconds, play or pause, forward
//! ten seconds, the progress, the sound, the rate and the tracks, each following the player; and
//! the way back to the library. Fluent's buttons, on a dark see-through background, as players
//! put their controls over the picture.

use std::rc::Rc;

use waft_embedder::{Color, PlayerState};
use waft_foundation::{App, Handle, Listenable, Listener};
use waft_widgets::{
    BorderRadius, BoxDecoration, DecoratedBox, EdgeInsetsGeometry, Expanded, IntoWidget,
    LayoutBuilder, ListenableBuilder, MainAxisSize, Padding, Player, Row, SizedBox, WidgetRef,
};
use waft_winui::{Button, ButtonStyle, MenuFlyoutItem, TextBlockStyle};

use super::menu_button::MenuButton;
use super::progress::Progress;
use super::volume::volume;
use crate::commands::{self, RATES, Skip};
use crate::icons::{self, icon};
use crate::text::{Tone, line};
use crate::tracks::{self, TrackChoice};

/// The bar's background: dark and see-through, as QuickTime's.
const BACKGROUND: Color = Color::from_argb(170, 30, 30, 30);

/// How wide the bar is, at the least, to show the volume's slider beside the seek bar.
const WIDE: f64 = 640.0;

/// The size of the bar's icons.
const ICON_SIZE: f64 = 20.0;

/// `player`'s controls, built again at each of its changes and of the room they have.
pub(crate) fn control_bar(player: Handle<Player>) -> WidgetRef {
    let listenable: Rc<dyn Listenable> = Rc::new(player);
    let controls = ListenableBuilder::new(listenable, move |app, _context, _child| {
        let state = player.state(app).clone();
        LayoutBuilder::new(move |_app, _context, constraints| {
            controls(player, &state, constraints.max_width >= WIDE)
        })
        .into_widget()
    });
    on_background(Padding::new(EdgeInsetsGeometry::symmetric(4.0, 8.0)).child(controls))
}

/// The way back to the library.
pub(crate) fn back_button(back: Listener) -> WidgetRef {
    let face = Row::new().main_axis_size(MainAxisSize::Min).children([
        icon(icons::BACK, ICON_SIZE).into_widget(),
        SizedBox::new().width(8.0).into_widget(),
        line("Library", TextBlockStyle::Body, Tone::Primary),
    ]);
    on_background(Button::new(face, back).style(ButtonStyle::Subtle))
}

/// `child` on the bar's background.
fn on_background<K>(child: impl IntoWidget<K>) -> WidgetRef {
    DecoratedBox::new(
        BoxDecoration::new()
            .color(BACKGROUND)
            .border_radius(BorderRadius::circular(12.0)),
    )
    .child(child)
    .into_widget()
}

/// The controls in a row, the volume's slider among them where the bar is `wide`.
fn controls(player: Handle<Player>, state: &PlayerState, wide: bool) -> WidgetRef {
    Row::new()
        .spacing(4.0)
        .children([
            icon_button(icons::SKIP_BACK_10, skip(player, Skip::Back)),
            play_pause_button(player, state),
            icon_button(icons::SKIP_FORWARD_10, skip(player, Skip::Forward)),
            Expanded::new(Progress::new(player)).into_widget(),
            volume(player, state, wide),
            rate_button(player, state),
            tracks_button(player, state),
        ])
        .into_widget()
}

/// Play while paused, pause while playing or waiting to.
fn play_pause_button(player: Handle<Player>, state: &PlayerState) -> WidgetRef {
    let glyph = if commands::is_paused(state) {
        icons::PLAY
    } else {
        icons::PAUSE
    };
    let toggle = Listener::new(move |app: &mut App| commands::play_or_pause(app, player));
    icon_button(glyph, toggle)
}

fn skip(player: Handle<Player>, skip: Skip) -> Listener {
    Listener::new(move |app: &mut App| commands::skip(app, player, skip))
}

/// The rate it plays at, which opens a menu of the rates.
fn rate_button(player: Handle<Player>, state: &PlayerState) -> WidgetRef {
    let rate = state.rate;
    let items = RATES
        .into_iter()
        .map(|choice| {
            let set = Listener::new(move |app: &mut App| player.set_rate(app, choice));
            MenuFlyoutItem::radio(rate_label(choice), (choice - rate).abs() < 0.01, set)
        })
        .collect();
    let face = line(rate_label(rate), TextBlockStyle::Body, Tone::Primary);
    MenuButton::new(face, items).into_widget()
}

/// The subtitles and audio to choose among, which opens a menu of them.
fn tracks_button(player: Handle<Player>, state: &PlayerState) -> WidgetRef {
    let subtitles = tracks::subtitle_choices(state)
        .iter()
        .map(|choice| track_item(player, choice))
        .collect();
    let mut items = vec![MenuFlyoutItem::sub_item("Subtitles", subtitles)];
    let audio: Vec<MenuFlyoutItem> = tracks::audio_choices(state)
        .iter()
        .map(|choice| track_item(player, choice))
        .collect();
    if !audio.is_empty() {
        items.push(MenuFlyoutItem::sub_item("Audio", audio));
    }
    MenuButton::new(icon(icons::SUBTITLES, ICON_SIZE).into_widget(), items).into_widget()
}

fn track_item(player: Handle<Player>, choice: &TrackChoice) -> MenuFlyoutItem {
    let pick = choice.pick.clone();
    let choose = Listener::new(move |app: &mut App| tracks::pick_track(app, player, &pick));
    MenuFlyoutItem::radio(choice.label.clone(), choice.chosen, choose)
}

/// `glyph`, a subtle button that does `pressed`.
pub(crate) fn icon_button(glyph: char, pressed: Listener) -> WidgetRef {
    Button::new(icon(glyph, ICON_SIZE), pressed)
        .style(ButtonStyle::Subtle)
        .into_widget()
}

/// A rate as players word it: `1×`, `1.25×`.
fn rate_label(rate: f64) -> String {
    format!("{}×", (rate * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_is_worded_as_players_word_it() {
        assert_eq!(rate_label(1.0), "1×");
        assert_eq!(rate_label(1.25), "1.25×");
        assert_eq!(rate_label(0.5), "0.5×");
    }
}
