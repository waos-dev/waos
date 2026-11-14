//! The app's state, as its pages and commands reach it: the viewfinder, the darkroom its looks
//! are drawn in, the roll of photos kept, and what the window says went wrong.

use waft::{App, Entity};
use waft_winui::InfoBarSeverity;

use crate::darkroom::Darkroom;
use crate::roll::Roll;
use crate::viewfinder::Viewfinder;

/// What the window says of a command, until the user closes it or the next is said.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Notice {
    pub(crate) severity: InfoBarSeverity,
    pub(crate) said: String,
}

#[derive(Clone)]
pub(crate) struct CameraApp {
    pub(crate) viewfinder: Entity<Viewfinder>,
    pub(crate) darkroom: Entity<Darkroom>,
    pub(crate) roll: Entity<Roll>,
    pub(crate) notice: Entity<Option<Notice>>,
}

impl CameraApp {
    /// The app as it starts: the camera starting, the darkroom opening, and the photos kept.
    pub(crate) fn open(app: &mut App) -> CameraApp {
        CameraApp {
            viewfinder: Viewfinder::open(app),
            darkroom: Darkroom::open(app),
            roll: Roll::open(app),
            notice: app.new_entity(|_cx| None),
        }
    }

    /// Has the window say `said`.
    pub(crate) fn say(&self, app: &mut App, severity: InfoBarSeverity, said: impl Into<String>) {
        let notice = Notice {
            severity,
            said: said.into(),
        };
        self.notice.update(app, |shown, cx| {
            *shown = Some(notice);
            cx.notify();
        });
    }

    /// Takes away what the window says.
    pub(crate) fn dismiss_notice(&self, app: &mut App) {
        self.notice.update(app, |shown, cx| {
            *shown = None;
            cx.notify();
        });
    }
}

impl std::fmt::Debug for CameraApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CameraApp").finish_non_exhaustive()
    }
}
