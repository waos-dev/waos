//! The modifier keys held now, as the framework's keyboard keeps them.

use waft_foundation::App;
use waft_services::HardwareKeyboard;

use crate::keys::Modifiers;

pub(crate) fn held_modifiers(app: &mut App) -> Modifiers {
    let keyboard = HardwareKeyboard::instance(app);
    Modifiers {
        shift: keyboard.is_shift_pressed(app),
        control: keyboard.is_control_pressed(app),
        alt: keyboard.is_alt_pressed(app),
        meta: keyboard.is_meta_pressed(app),
    }
}
