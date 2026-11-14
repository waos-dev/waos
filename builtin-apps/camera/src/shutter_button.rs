//! The shutter: a white disc in a white ring, as a camera's is, which takes a photo at a tap and
//! draws in a little while it is pressed; dimmed while there is no photo to take.

use std::fmt;

use waft::{
    App, Border, BorderStyle, BoxDecoration, BoxShape, BuildContext, Center, Color, DecoratedBox,
    GestureDetector, Handle, IntoWidget, Listener, Opacity, SizedBox, State, StateData,
    StatefulWidget, WidgetRef,
};

/// The ring's width across, the disc's, and the disc's while pressed, in logical points.
const RING: f64 = 64.0;
const DISC: f64 = 50.0;
const PRESSED_DISC: f64 = 44.0;

const WHITE: Color = Color::from_argb(255, 255, 255, 255);

/// How much of the shutter shows while it takes no photo.
const DIMMED: f64 = 0.35;

pub(crate) struct ShutterButton {
    /// What a tap does; none while there is no photo to take.
    pub(crate) press: Option<Listener>,
}

impl fmt::Debug for ShutterButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShutterButton")
            .field("enabled", &self.press.is_some())
            .finish()
    }
}

impl StatefulWidget for ShutterButton {
    type State = ShutterButtonState;

    fn create_state(&self) -> ShutterButtonState {
        ShutterButtonState {
            state: StateData::new(),
            pressed: false,
        }
    }
}

pub(crate) struct ShutterButtonState {
    state: StateData<ShutterButton>,
    pressed: bool,
}

impl ShutterButtonState {
    fn set_pressed(self: Handle<Self>, app: &mut App, pressed: bool) {
        self.set_state(app, |state| state.pressed = pressed);
    }
}

impl State for ShutterButtonState {
    type Widget = ShutterButton;
    waft::state_accessors!();

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let disc = if app.get(self).pressed {
            PRESSED_DISC
        } else {
            DISC
        };
        let shutter = ring_around(disc);
        let Some(press) = self.widget(app).press.clone() else {
            return Opacity::new(DIMMED).child(shutter).into_widget();
        };
        GestureDetector::new()
            .on_tap_down(std::rc::Rc::new(move |app, _details| {
                self.set_pressed(app, true)
            }))
            .on_tap_up(std::rc::Rc::new(move |app, _details| {
                self.set_pressed(app, false)
            }))
            .on_tap_cancel(Listener::new(move |app| self.set_pressed(app, false)))
            .on_tap(press)
            .child(shutter)
            .into_widget()
    }
}

/// The ring, with a disc `disc` points across in its middle.
fn ring_around(disc: f64) -> WidgetRef {
    let round = |decoration: BoxDecoration| decoration.shape(BoxShape::Circle);
    let middle = SizedBox::new()
        .width(disc)
        .height(disc)
        .child(DecoratedBox::new(round(BoxDecoration::new().color(WHITE))));
    let ring =
        round(BoxDecoration::new().border(Border::all(WHITE, 3.0, BorderStyle::Solid, -1.0)));
    SizedBox::new()
        .width(RING)
        .height(RING)
        .child(DecoratedBox::new(ring).child(Center::new().child(middle)))
        .into_widget()
}
