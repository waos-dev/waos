//! The quiet banner above the home's grid that says who is using the microphone or the camera
//! (`state::recordings`), in each layout's colours: a sentence for each device on a faint rounded
//! ground, a device of WaOS's own said simulated, nothing while no program records.

use waft::{
    App, BuildContext, Container, EdgeInsetsGeometry, Entity, IntoWidget, Text, TextStyle,
    WidgetRef,
};
use waft_cupertino::CupertinoColors;
use waft_winui::{TextBlockStyle, ThemeResources};
use waos::RecordingDevice;

use super::home::banner_ground;
use super::home::cupertino::resolved;
use super::words;
use crate::state::recordings::Recordings;

/// How far the banner's sentence sits from its edges, and how round its corners are.
const PADDING: f64 = 12.0;
const CUPERTINO_RADIUS: f64 = 10.0;
const FLUENT_RADIUS: f64 = 8.0;

/// What the library says of the programs recording now; none while none does.
pub(crate) fn recording_said(recordings: &Entity<Recordings>, app: &App) -> Option<String> {
    let recordings = recordings.read(app);
    let using: Vec<(RecordingDevice, Vec<String>)> = [
        RecordingDevice::Microphone,
        RecordingDevice::Camera,
        RecordingDevice::SimulatedMicrophone,
        RecordingDevice::SimulatedCamera,
    ]
    .into_iter()
    .map(|device| (device, recordings.names(device)))
    .collect();
    words::recording(&using)
}

/// The banner in Cupertino's colours, while a program records.
pub(crate) fn cupertino_recording(
    recordings: &Entity<Recordings>,
    app: &mut App,
    context: BuildContext,
) -> Option<WidgetRef> {
    let said = recording_said(recordings, app)?;
    let style = TextStyle::new()
        .color(resolved(CupertinoColors::SECONDARY_LABEL, app, context))
        .font_size(15.0);
    let ground = resolved(CupertinoColors::TERTIARY_SYSTEM_FILL, app, context);
    Some(banner(
        Text::new(said).style(style),
        ground,
        CUPERTINO_RADIUS,
    ))
}

/// The banner in Fluent's colours, while a program records.
pub(crate) fn fluent_recording(
    recordings: &Entity<Recordings>,
    app: &mut App,
    context: BuildContext,
) -> Option<WidgetRef> {
    let said = recording_said(recordings, app)?;
    let resources = ThemeResources::of(app, context);
    let style = TextBlockStyle::Body.text_style(&resources, resources.text_fill_color_secondary);
    Some(banner(
        Text::new(said).style(style),
        resources.card_background_fill_color_default,
        FLUENT_RADIUS,
    ))
}

fn banner(said: Text, ground: waft::Color, radius: f64) -> WidgetRef {
    Container::new()
        .decoration(banner_ground(ground, radius))
        .padding(EdgeInsetsGeometry::all(PADDING))
        .child(said)
        .into_widget()
}
