//! The controls: the preset chosen from a list, the switch that sustains notes, the tone's
//! sliders, each under its name and its value, and the buttons that move the keyboard an octave.

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, Expanded, IntoWidget, Listener, MainAxisSize,
    Row, SizedBox, StatelessWidget, Text, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, ComboBox, ComboBoxItem, FluentSymbol, FontIcon, Slider, TextBlockStyle,
    ThemeResources, ToggleButton,
};

use crate::control::Control;
use crate::instrument::Instrument;
use crate::presets::Preset;

/// The room between two sliders.
const SLIDER_GAP: f64 = 20.0;

/// How fine a slider's steps are, as parts of its travel.
const SLIDER_STEP: f64 = 0.001;

/// The size of the octave buttons' arrows, and the room the keyboard's octave is written in.
const ARROW_SIZE: f64 = 14.0;
const OCTAVE_WIDTH: f64 = 64.0;

/// The list the preset is chosen from.
pub(crate) struct PresetList {
    instrument: Instrument,
}

impl PresetList {
    pub(crate) fn new(instrument: Instrument) -> PresetList {
        PresetList { instrument }
    }
}

impl std::fmt::Debug for PresetList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresetList").finish_non_exhaustive()
    }
}

impl StatelessWidget for PresetList {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let chosen = self.instrument.playing().read(app).preset;
        let items = Preset::ALL
            .into_iter()
            .map(|preset| ComboBoxItem::text(preset.name(), preset.name()))
            .collect();
        let index = Preset::ALL.iter().position(|preset| *preset == chosen);
        let instrument = self.instrument.clone();
        ComboBox::new(items, index, move |app, change| {
            if let Some(preset) = change
                .selected_index()
                .and_then(|index| Preset::ALL.get(index))
            {
                instrument.choose_preset(app, *preset);
            }
        })
        // Typing plays the keyboard, the list open or not.
        .is_text_search_enabled(false)
        .into_widget()
    }
}

/// The switch that keeps notes sounding after their keys are let go, for a chord built a key at
/// a time, and for a hand free to move the sliders while it sounds.
pub(crate) struct SustainSwitch {
    instrument: Instrument,
}

impl SustainSwitch {
    pub(crate) fn new(instrument: Instrument) -> SustainSwitch {
        SustainSwitch { instrument }
    }
}

impl std::fmt::Debug for SustainSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SustainSwitch").finish_non_exhaustive()
    }
}

impl StatelessWidget for SustainSwitch {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let sustaining = self.instrument.playing().read(app).held.is_sustaining();
        let instrument = self.instrument.clone();
        ToggleButton::text("Sustain", Some(sustaining), move |app, sustaining| {
            instrument.sustain(app, sustaining == Some(true));
        })
        .into_widget()
    }
}

/// The tone's sliders, side by side, or one under another where `stacked`.
pub(crate) struct ToneSliders {
    instrument: Instrument,
    stacked: bool,
}

impl ToneSliders {
    pub(crate) fn new(instrument: Instrument, stacked: bool) -> ToneSliders {
        ToneSliders {
            instrument,
            stacked,
        }
    }
}

impl std::fmt::Debug for ToneSliders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToneSliders")
            .field("stacked", &self.stacked)
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for ToneSliders {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let tone = self.instrument.playing().read(app).tone;
        let sliders: Vec<WidgetRef> = Control::ALL
            .into_iter()
            .map(|control| self.slider(app, context, control, control.of(&tone)))
            .collect();
        if self.stacked {
            return Column::new()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min)
                .children(sliders)
                .into_widget();
        }
        let gap = || SizedBox::new().width(SLIDER_GAP).into_widget();
        let spaced = sliders
            .into_iter()
            .flat_map(|slider| [gap(), Expanded::new(slider).into_widget()])
            .skip(1);
        Row::new()
            .children(spaced.collect::<Vec<_>>())
            .into_widget()
    }
}

impl ToneSliders {
    /// `control`'s slider at `value`, under its name and the value as its user reads it.
    fn slider(
        &self,
        app: &mut App,
        context: BuildContext,
        control: Control,
        value: f32,
    ) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let name = Text::new(control.name())
            .style(TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_primary));
        let shown = Text::new(control.shown(value)).style(
            TextBlockStyle::Caption.text_style(&resources, resources.text_fill_color_secondary),
        );
        let header = Row::new().children([Expanded::new(name).into_widget(), shown.into_widget()]);
        let instrument = self.instrument.clone();
        Slider::new(
            control.position(value),
            move |app: &mut App, position: f64| {
                instrument.move_control(app, control, control.value_at(position));
            },
        )
        .maximum(1.0)
        .step_frequency(SLIDER_STEP)
        .small_change(0.02)
        .is_thumb_tool_tip_enabled(false)
        .header(header)
        .into_widget()
    }
}

/// The buttons that move the keyboard an octave down and up, around the C it starts at.
pub(crate) struct OctaveButtons {
    instrument: Instrument,
}

impl OctaveButtons {
    pub(crate) fn new(instrument: Instrument) -> OctaveButtons {
        OctaveButtons { instrument }
    }
}

impl std::fmt::Debug for OctaveButtons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OctaveButtons").finish_non_exhaustive()
    }
}

impl StatelessWidget for OctaveButtons {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let playing = self.instrument.playing().read(app);
        let (lowest, can_go_down, can_go_up) = (
            playing.lowest,
            playing.can_shift_octaves(-1),
            playing.can_shift_octaves(1),
        );
        let starts_at = Text::new(format!("Octave {}", lowest.octave())).style(
            TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary),
        );
        Row::new()
            .main_axis_size(MainAxisSize::Min)
            .children([
                self.arrow(FluentSymbol::ChevronLeft, -1, can_go_down),
                SizedBox::new()
                    .width(OCTAVE_WIDTH)
                    .child(waft::Center::new().child(starts_at))
                    .into_widget(),
                self.arrow(FluentSymbol::ChevronRight, 1, can_go_up),
            ])
            .into_widget()
    }
}

impl OctaveButtons {
    /// A button showing `arrow` that moves the keyboard by `octaves`, where it `can`.
    fn arrow(&self, arrow: FluentSymbol, octaves: i8, can: bool) -> WidgetRef {
        let instrument = self.instrument.clone();
        let shift = Listener::new(move |app: &mut App| instrument.shift_octaves(app, octaves));
        Button::new(FontIcon::symbol(arrow).font_size(ARROW_SIZE), shift)
            .style(ButtonStyle::Subtle)
            .is_enabled(can)
            // The focus stays where typing plays from.
            .is_tab_stop(false)
            .into_widget()
    }
}
