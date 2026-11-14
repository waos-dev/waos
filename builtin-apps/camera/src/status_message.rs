//! What the window says where the camera's picture would be, while there is none: that the
//! camera is starting, that WaOS is asking the user for it, that there is none, that the user
//! refused it and where to allow it, that the system paused it, or why it does not run.

use waft::ui::CameraInterruption;
use waft::{
    App, BoxConstraints, BuildContext, Center, Column, ConstrainedBox, CrossAxisAlignment,
    EdgeInsetsGeometry, IntoWidget, MainAxisSize, Padding, SizedBox, StatelessWidget, Text,
    TextAlign, WidgetRef,
};
use waft_winui::{TextBlockStyle, ThemeResources};

use crate::icons::{self, icon};
use crate::viewfinder::Status;

/// The widest the words run, in logical points.
const WIDEST: f64 = 380.0;

/// An icon over a line that says what is, and a line that says what to do about it.
#[derive(Debug)]
pub(crate) struct StatusMessage {
    glyph: char,
    title: String,
    advice: String,
}

impl StatusMessage {
    fn new(glyph: char, title: &str, advice: &str) -> StatusMessage {
        StatusMessage {
            glyph,
            title: title.to_owned(),
            advice: advice.to_owned(),
        }
    }

    /// What is said while the viewfinder stands at `status`; nothing while the camera runs.
    pub(crate) fn of(status: &Status) -> Option<StatusMessage> {
        let message = match status {
            Status::Running(_) => return None,
            Status::Starting | Status::Resting => {
                StatusMessage::new(icons::CAMERA, "Starting the camera…", "")
            }
            Status::Asking => StatusMessage::new(
                icons::CAMERA,
                "Allow the camera",
                "WaOS is asking whether Camera may use the camera. Its picture shows here once you allow it.",
            ),
            Status::NoCamera => StatusMessage::new(
                icons::CAMERA_OFF,
                "No camera",
                "Connect a camera, and its picture shows here.",
            ),
            Status::NotAllowed => StatusMessage::new(
                icons::LOCK,
                "Camera may not use the camera",
                "Allow the camera for this app in WaOS's settings, and its picture shows here.",
            ),
            Status::Failed(why) => {
                StatusMessage::new(icons::CAMERA_OFF, "The camera is not running", why)
            }
        };
        Some(message)
    }

    /// What is said while the system keeps the running camera from giving frames, for the reason
    /// it gives where it gives one.
    pub(crate) fn interrupted(why: Option<CameraInterruption>) -> StatusMessage {
        let advice = match why {
            Some(CameraInterruption::InUseByAnotherApp) => "Another app is using the camera.",
            Some(CameraInterruption::Background) => "It runs again once Camera is in front.",
            Some(CameraInterruption::SystemPressure) => {
                "The system is too hot or too busy to run it for now."
            }
            None => "The system stopped it for now, and starts it again by itself.",
        };
        StatusMessage::new(icons::CAMERA_OFF, "The camera is paused", advice)
    }

    /// What is said where the app has no device to draw the camera's picture with.
    pub(crate) fn no_drawing(why: &str) -> StatusMessage {
        StatusMessage::new(
            icons::CAMERA_OFF,
            "The camera's picture cannot be drawn",
            why,
        )
    }
}

impl StatelessWidget for StatusMessage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let r = ThemeResources::of(app, context);
        let title = Text::new(self.title.clone())
            .style(TextBlockStyle::Subtitle.text_style(&r, r.text_fill_color_primary))
            .text_align(TextAlign::Center);
        let advice = Text::new(self.advice.clone())
            .style(TextBlockStyle::Body.text_style(&r, r.text_fill_color_secondary))
            .text_align(TextAlign::Center);
        let words = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children([
                icon(self.glyph, 40.0)
                    .foreground(r.text_fill_color_secondary)
                    .into_widget(),
                SizedBox::new().height(16.0).into_widget(),
                title.into_widget(),
                SizedBox::new().height(6.0).into_widget(),
                advice.into_widget(),
            ]);
        let fitted = ConstrainedBox::new(BoxConstraints::new().max_width(WIDEST)).child(words);
        Center::new()
            .child(Padding::new(EdgeInsetsGeometry::all(24.0)).child(fitted))
            .into_widget()
    }
}
