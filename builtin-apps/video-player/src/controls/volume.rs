//! The sound: a speaker that mutes and unmutes, and Fluent's slider of the volume, which the
//! player reads back, since the system may change either itself.

use waft_embedder::PlayerState;
use waft_foundation::{App, Handle, Listener};
use waft_widgets::{IntoWidget, MainAxisSize, Player, Row, SizedBox, WidgetRef};
use waft_winui::Slider;

use super::bar::icon_button;
use crate::commands;
use crate::icons;

/// How long the volume's slider is.
const SLIDER_WIDTH: f64 = 96.0;

/// `player`'s mute switch and, `with_slider`, its volume's slider, as `state` has them.
pub(crate) fn volume(player: Handle<Player>, state: &PlayerState, with_slider: bool) -> WidgetRef {
    if !with_slider {
        return mute_button(player, state);
    }
    let shown = if state.muted { 0.0 } else { state.volume };
    let slider = Slider::new(shown, move |app: &mut App, volume: f64| {
        player.set_volume(app, volume);
        if player.state(app).muted {
            player.set_muted(app, false);
        }
    })
    .maximum(1.0)
    .step_frequency(0.01)
    .small_change(0.05)
    .is_thumb_tool_tip_enabled(false);
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .children([
            mute_button(player, state),
            SizedBox::new()
                .width(SLIDER_WIDTH)
                .child(slider)
                .into_widget(),
        ])
        .into_widget()
}

/// The speaker, showing whether the sound is off, which mutes or unmutes.
fn mute_button(player: Handle<Player>, state: &PlayerState) -> WidgetRef {
    let toggle = Listener::new(move |app: &mut App| commands::toggle_mute(app, player));
    let glyph = if state.muted || state.volume == 0.0 {
        icons::SPEAKER_MUTE
    } else {
        icons::SPEAKER
    };
    icon_button(glyph, toggle)
}
