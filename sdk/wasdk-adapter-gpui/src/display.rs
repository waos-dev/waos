//! The system's screens as GPUI's displays.
//!
//! A screen's frame and visible frame are in the system's logical coordinates, as GPUI's display
//! bounds are. GPUI persists a display by its UUID to put windows back; wasdk gives a screen an id
//! and a name, so the UUID is made from both, stable for as long as the system keeps them.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use anyhow::Result;
use gpui::{Bounds, DisplayId, Pixels, PlatformDisplay, point, px, size};
use uuid::Uuid;
use wasdk::screens::Screen;
use wasdk::windows::Rect;

#[derive(Debug)]
pub(crate) struct WasdkDisplay {
    id: DisplayId,
    uuid: Uuid,
    bounds: Bounds<Pixels>,
    visible_bounds: Bounds<Pixels>,
    refresh_rate: u32,
}

impl WasdkDisplay {
    /// The display `screen` is, as it is now.
    pub(crate) fn of(screen: &Screen) -> WasdkDisplay {
        WasdkDisplay {
            id: DisplayId::new(screen.id()),
            uuid: uuid_of(screen.id(), &screen.name()),
            bounds: bounds_of(screen.frame()),
            visible_bounds: bounds_of(screen.visible_frame()),
            refresh_rate: screen.refresh_rate(),
        }
    }

    /// Where a window goes when the system has no screens to say: a display of the usual size.
    pub(crate) fn assumed() -> WasdkDisplay {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)));
        WasdkDisplay {
            id: DisplayId::new(0),
            uuid: Uuid::nil(),
            bounds,
            visible_bounds: bounds,
            refresh_rate: 60,
        }
    }
}

impl PlatformDisplay for WasdkDisplay {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn uuid(&self) -> Result<Uuid> {
        Ok(self.uuid)
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    fn visible_bounds(&self) -> Bounds<Pixels> {
        self.visible_bounds
    }

    fn refresh_interval(&self) -> Option<Duration> {
        (self.refresh_rate > 0).then(|| Duration::from_secs_f64(1.0 / f64::from(self.refresh_rate)))
    }
}

pub(crate) fn bounds_of(rect: Rect) -> Bounds<Pixels> {
    Bounds::new(
        point(px(rect.origin.x as f32), px(rect.origin.y as f32)),
        size(px(rect.size.width as f32), px(rect.size.height as f32)),
    )
}

fn uuid_of(id: u64, name: &str) -> Uuid {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    Uuid::from_u64_pair(id, hasher.finish())
}
