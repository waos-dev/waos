//! `wasdk:platform/screens`: the screens the system shows views on, as the shell reports them
//! and every program reads them.
//!
//! One [`Screens`] serves every program, and every clone is the same one. The shell sets the
//! whole list each time it changes, and what was added, removed or changed is worked out here.
//! The WIT's `screen` is a resource; [`Screen`] is the plain value of what its getters answer,
//! and the code serving the interface, [`host`], makes the handles. The invariant: a change is
//! made to the list and announced under one lock, so what `list()` answers and what `changes()`
//! delivers are one history.

pub(crate) mod host;

use std::sync::{Arc, Mutex};

use futures::channel::mpsc;

use crate::bindings::wasdk::platform::screens::{PixelSize, Rect, ScreenId};
use crate::channel::Channel;
use crate::sync::locked;

/// One screen as the system has it now: what the WIT's `screen` answers.
#[derive(Clone, Debug)]
pub struct Screen {
    pub id: ScreenId,
    /// The name the system shows for it; empty where the system has none.
    pub name: String,
    /// Its size now, in physical pixels.
    pub size: PixelSize,
    /// Physical pixels per logical pixel on it now.
    pub scale: f64,
    /// How often it refreshes now, in millihertz: 60 000 for 60 Hz. 0 where the system does
    /// not say.
    pub refresh_rate: u32,
    /// Where it is on the desktop, in logical points from the top-left of the main screen, y
    /// down: the coordinates a window's frame is in.
    pub frame: Rect,
    /// What the system's own bars leave free, in the same coordinates; the whole frame where the
    /// system cannot say.
    pub visible_frame: Rect,
}

/// Two screens are the same when everything the program reads of them is: what a change of
/// one is told from.
impl PartialEq for Screen {
    fn eq(&self, other: &Screen) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.size.width == other.size.width
            && self.size.height == other.size.height
            && self.scale == other.scale
            && self.refresh_rate == other.refresh_rate
            && same_rect(&self.frame, &other.frame)
            && same_rect(&self.visible_frame, &other.visible_frame)
    }
}

fn same_rect(a: &Rect, b: &Rect) -> bool {
    a.origin.x == b.origin.x
        && a.origin.y == b.origin.y
        && a.size.width == b.size.width
        && a.size.height == b.size.height
}

/// A change to the screens, in the order the changes happened: the WIT's `screen-change` with
/// the screen itself in place of its handle.
#[derive(Clone, Debug)]
pub(crate) enum ScreenChange {
    /// A screen the system now has.
    Added(Screen),
    /// A screen the system no longer has.
    Removed(ScreenId),
    /// A screen whose size, scale, refresh rate, frame or visible frame changed.
    Changed(ScreenId),
}

/// The screens the system has, shared by the shell, which reports into them, and every
/// program, which reads them.
#[derive(Clone)]
pub(crate) struct Screens(Arc<ScreenList>);

struct ScreenList {
    screens: Mutex<Vec<Screen>>,
    changes: Channel<ScreenChange>,
}

impl Screens {
    /// The screens the system has at the start.
    pub(crate) fn new(screens: Vec<Screen>) -> Screens {
        Screens(Arc::new(ScreenList {
            screens: Mutex::new(screens),
            changes: Channel::default(),
        }))
    }

    /// The screens the system has now.
    pub(crate) fn list(&self) -> Vec<Screen> {
        locked(&self.0.screens).clone()
    }

    /// What changes among the screens from now on, on a stream of its own.
    pub(crate) fn changes(&self) -> mpsc::UnboundedReceiver<ScreenChange> {
        self.0.changes.subscribe()
    }

    /// The screen `id` as the system has it now; none for one it does not have.
    pub(crate) fn find(&self, id: ScreenId) -> Option<Screen> {
        locked(&self.0.screens)
            .iter()
            .find(|screen| screen.id == id)
            .cloned()
    }

    /// The system has `screens` now, the whole list: the programs hear which were added,
    /// which removed, and which changed in size, scale, refresh rate, frame or visible frame.
    pub(crate) fn set(&self, screens: Vec<Screen>) {
        let mut current = locked(&self.0.screens);
        for known in current.iter() {
            if !screens.iter().any(|screen| screen.id == known.id) {
                self.0.changes.send(ScreenChange::Removed(known.id));
            }
        }
        for screen in &screens {
            match current.iter().find(|known| known.id == screen.id) {
                None => self.0.changes.send(ScreenChange::Added(screen.clone())),
                Some(known) if known != screen => {
                    self.0.changes.send(ScreenChange::Changed(screen.id));
                }
                Some(_) => {}
            }
        }
        *current = screens;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::bindings::wasdk::platform::windows::{Point, Size};

    use super::*;

    /// A screen the system says refreshes at `refresh_rate` millihertz, or says no rate of at 0.
    pub(crate) fn screen_refreshing_at(id: ScreenId, refresh_rate: u32) -> Screen {
        Screen {
            refresh_rate,
            ..screen(id, 2)
        }
    }

    /// A screen `width` physical pixels wide, at the top-left of the desktop, wholly free.
    fn screen(id: ScreenId, width: u32) -> Screen {
        let frame = rect(0.0, 0.0, f64::from(width) / 2.0, 0.5);
        Screen {
            id,
            name: "Built-in Retina Display".into(),
            size: PixelSize { width, height: 1 },
            scale: 2.0,
            refresh_rate: 60_000,
            frame,
            visible_frame: frame,
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            origin: Point { x, y },
            size: Size { width, height },
        }
    }

    /// What `changes` has delivered so far.
    fn heard(changes: &mut mpsc::UnboundedReceiver<ScreenChange>) -> Vec<ScreenChange> {
        std::iter::from_fn(|| changes.try_recv().ok()).collect()
    }

    #[test]
    fn a_screen_added_is_listed_and_announced() {
        let screens = Screens::new(vec![screen(1, 10)]);
        let mut changes = screens.changes();
        screens.set(vec![screen(1, 10), screen(2, 20)]);
        let ids: Vec<_> = screens.list().iter().map(|screen| screen.id).collect();
        assert_eq!(ids, [1, 2]);
        assert!(matches!(
            &heard(&mut changes)[..],
            [ScreenChange::Added(added)] if added.id == 2
        ));
    }

    #[test]
    fn a_screen_removed_is_gone_and_announced() {
        let screens = Screens::new(vec![screen(1, 10), screen(2, 20)]);
        let mut changes = screens.changes();
        screens.set(vec![screen(2, 20)]);
        assert_eq!(screens.list().len(), 1);
        assert!(screens.find(1).is_none() && screens.find(2).is_some());
        assert!(matches!(
            &heard(&mut changes)[..],
            [ScreenChange::Removed(1)]
        ));
    }

    #[test]
    fn a_screen_changed_is_read_back_and_announced() {
        let screens = Screens::new(vec![screen(1, 10)]);
        let mut changes = screens.changes();
        screens.set(vec![screen(1, 30)]);
        assert_eq!(screens.list()[0].size.width, 30);
        assert!(matches!(
            &heard(&mut changes)[..],
            [ScreenChange::Changed(1)]
        ));
    }

    #[test]
    fn a_screen_moved_on_the_desktop_is_read_back_and_announced() {
        let screens = Screens::new(vec![screen(1, 10)]);
        let mut changes = screens.changes();
        let mut moved = screen(1, 10);
        moved.frame.origin.x = -5.0;
        moved.visible_frame.origin.x = -5.0;
        screens.set(vec![moved]);
        assert_eq!(screens.list()[0].frame.origin.x, -5.0);
        assert!(matches!(
            &heard(&mut changes)[..],
            [ScreenChange::Changed(1)]
        ));
    }

    #[test]
    fn a_screen_whose_bars_leave_another_area_free_is_read_back_and_announced() {
        let screens = Screens::new(vec![screen(1, 10)]);
        let mut changes = screens.changes();
        let mut docked = screen(1, 10);
        docked.visible_frame = rect(0.0, 0.0, 5.0, 0.25);
        screens.set(vec![docked]);
        assert_eq!(screens.list()[0].visible_frame.size.height, 0.25);
        assert!(matches!(
            &heard(&mut changes)[..],
            [ScreenChange::Changed(1)]
        ));
    }

    #[test]
    fn the_same_list_again_is_no_change() {
        let screens = Screens::new(vec![screen(1, 10)]);
        let mut changes = screens.changes();
        screens.set(vec![screen(1, 10)]);
        assert!(heard(&mut changes).is_empty());
    }

    #[test]
    fn one_list_announces_every_kind_of_change() {
        let screens = Screens::new(vec![screen(1, 10), screen(2, 20)]);
        let mut changes = screens.changes();
        screens.set(vec![screen(2, 25), screen(3, 30)]);
        let heard = heard(&mut changes);
        assert!(matches!(
            &heard[..],
            [ScreenChange::Removed(1), ScreenChange::Changed(2), ScreenChange::Added(added)]
                if added.id == 3
        ));
    }

    /// The shell reports from its thread and every program's thread reads.
    #[test]
    fn the_screens_cross_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<Screens>();
    }
}
