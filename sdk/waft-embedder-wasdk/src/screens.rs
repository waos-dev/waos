//! The screens as Waft's [`Screens`]: each monitor's id, name, frame, visible frame, scale and
//! refresh rate, as `screens` lists them; where the pointer is, no host here can say.

use std::cell::RefCell;

use waft_embedder::{Offset, Screen, ScreenId, Screens};
use wasdk::screens;

use crate::windows::rect_of;

/// The screens as `screens` answered them last.
pub(crate) struct WasdkScreens {
    screens: RefCell<Vec<Screen>>,
}

impl WasdkScreens {
    /// The screens the system has now.
    pub(crate) fn new() -> WasdkScreens {
        WasdkScreens {
            screens: RefCell::new(current_screens()),
        }
    }

    /// The screens changed: reads them again.
    pub(crate) fn refresh(&self) {
        *self.screens.borrow_mut() = current_screens();
    }
}

impl Screens for WasdkScreens {
    fn screens(&self) -> Vec<Screen> {
        self.screens.borrow().clone()
    }

    fn pointer_location(&self) -> Option<Offset> {
        None
    }
}

fn current_screens() -> Vec<Screen> {
    screens::screens().iter().map(screen_of).collect()
}

/// `screen` as Waft's.
fn screen_of(screen: &screens::Screen) -> Screen {
    Screen {
        id: ScreenId(screen.id()),
        name: screen.name(),
        frame: rect_of(screen.frame()),
        visible_frame: rect_of(screen.visible_frame()),
        scale: screen.scale(),
        refresh_rate: refresh_rate_of(screen.refresh_rate()),
    }
}

/// Hertz for `screens`' millihertz, where 0 is a system that does not say.
fn refresh_rate_of(millihertz: u32) -> Option<f64> {
    (millihertz != 0).then(|| f64::from(millihertz) / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refresh_rate_is_hertz_and_none_where_the_system_does_not_say() {
        assert_eq!(refresh_rate_of(60_000), Some(60.0));
        assert_eq!(refresh_rate_of(119_880), Some(119.88));
        assert_eq!(refresh_rate_of(0), None);
    }
}
