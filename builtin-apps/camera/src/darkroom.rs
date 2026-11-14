//! The darkroom: where the app's painter is once WaOS has given the program a WebGPU device of
//! its own, which it asks for as the app starts. The live picture and a photo's developing both
//! draw with the one painter.

use std::rc::Rc;

use waft::{App, Entity};

use crate::look_painter::LookPainter;

pub(crate) enum Darkroom {
    /// The device is being asked for.
    Opening,
    Open(Rc<LookPainter>),
    /// There is no device, for this reason.
    Shut(String),
}

impl Darkroom {
    /// Asks for the app's device, and answers the darkroom that opens once it is there.
    pub(crate) fn open(app: &mut App) -> Entity<Darkroom> {
        let darkroom = app.new_entity(|_cx| Darkroom::Opening);
        let opening = darkroom.clone();
        app.spawn(async move |cx| {
            let opened = match LookPainter::new().await {
                Ok(painter) => Darkroom::Open(Rc::new(painter)),
                Err(why) => Darkroom::Shut(why),
            };
            cx.update(|app| {
                opening.update(app, |darkroom, cx| {
                    *darkroom = opened;
                    cx.notify();
                });
            });
        });
        darkroom
    }

    /// The painter, once the darkroom is open.
    pub(crate) fn painter(&self) -> Option<Rc<LookPainter>> {
        match self {
            Darkroom::Open(painter) => Some(Rc::clone(painter)),
            Darkroom::Opening | Darkroom::Shut(_) => None,
        }
    }
}
