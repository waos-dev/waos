//! What the program chosen does as it happens, while one of its panels shows: the streams of that
//! panel held, and what they told, a panel's rows each.
//!
//! The invariant: only the panel shown is listened to, so nothing is made for a panel nobody looks
//! at (the trace's streams are made as they are asked for); showing another panel or another
//! program starts a new listening, and a stream of an older one is let go at its next event.

use waft::{App, AsyncApp, Entity};
use wasdk::StreamReader;
use wasdk::inspect::active_uses;
use wasdk::inspect::devices::Device;
use wasdk::inspect::trace;

use crate::inspector::{Inspector, Tab};
use crate::panels::Panel;
use crate::panels::devices::{Capturing, DeviceLog};
use crate::panels::files::FileLog;
use crate::panels::in_use::InUse;
use crate::panels::network::NetworkLog;
use crate::panels::output::OutputLog;

#[derive(Debug, Default)]
pub(crate) struct Live {
    /// Which listening this is: a listening that finds another has started stops.
    listening: u64,
    /// Whether the program runs, so there is anything to hear.
    pub(crate) heard: bool,
    network: NetworkLog,
    files: FileLog,
    devices: DeviceLog,
    output: OutputLog,
    in_use: InUse,
}

impl Live {
    /// The panel of `tab`; none for the report.
    pub(crate) fn panel(&self, tab: Tab) -> Option<&dyn Panel> {
        match tab {
            Tab::Report => None,
            Tab::Network => Some(&self.network),
            Tab::Files => Some(&self.files),
            Tab::Devices => Some(&self.devices),
            Tab::Output => Some(&self.output),
            Tab::InUse => Some(&self.in_use),
        }
    }

    /// Stops the listening, forgetting what it heard; answers the one that may start next.
    pub(crate) fn stop(&mut self) -> u64 {
        *self = Live {
            listening: self.listening + 1,
            ..Live::default()
        };
        self.listening
    }
}

/// Listens to `tab`'s streams of the program `id` as the listening `listening`, for as long as it
/// is the one wanted and the program runs; hears nothing of a program that does not run.
pub(crate) fn listen(
    app: &mut App,
    inspector: &Entity<Inspector>,
    (device, id): (&Device, &str),
    tab: Tab,
    listening: u64,
) {
    let heard = match tab {
        Tab::Report => false,
        Tab::Network => hear(
            app,
            inspector,
            listening,
            trace::network(device, id),
            |live, event| {
                live.network.add(&event);
            },
        ),
        Tab::Files => hear(
            app,
            inspector,
            listening,
            trace::files(device, id),
            |live, event| {
                live.files.add(&event);
            },
        ),
        Tab::Devices => listen_to_devices(app, inspector, (device, id), listening),
        Tab::Output => hear(
            app,
            inspector,
            listening,
            trace::output(device, id),
            |live, chunk| {
                live.output.add(&chunk);
            },
        ),
        Tab::InUse => listen_to_uses(app, inspector, (device, id), listening),
    };
    if heard {
        inspector.update(app, |inspector, cx| {
            if inspector.live.listening == listening {
                inspector.live.heard = true;
                cx.notify();
            }
        });
    }
}

/// The camera's, the microphone's and the machines' streams, into the one panel.
fn listen_to_devices(
    app: &mut App,
    inspector: &Entity<Inspector>,
    (device, id): (&Device, &str),
    listening: u64,
) -> bool {
    let camera = hear(
        app,
        inspector,
        listening,
        trace::camera(device, id),
        |live, event| {
            live.devices.add_capture(Capturing::Camera, &event);
        },
    );
    let microphone = hear(
        app,
        inspector,
        listening,
        trace::microphone(device, id),
        |live, event| {
            live.devices.add_capture(Capturing::Microphone, &event);
        },
    );
    let machines = hear(
        app,
        inspector,
        listening,
        trace::machines(device, id),
        |live, event| {
            live.devices.add_machine(&event);
        },
    );
    camera || microphone || machines
}

/// What the program uses now, then each change.
fn listen_to_uses(
    app: &mut App,
    inspector: &Entity<Inspector>,
    (device, id): (&Device, &str),
    listening: u64,
) -> bool {
    let Some((now, changes)) = active_uses::active_uses(device, id) else {
        return false;
    };
    if !told(app, inspector, listening, |live| live.in_use.began(now)) {
        return false;
    }
    hear(app, inspector, listening, Some(changes), |live, change| {
        live.in_use.change(change);
    })
}

/// Hears `events`, each told into the inspector's `Live` by `tell`, while `listening` is wanted;
/// answers whether there was a stream to hear.
fn hear<E: 'static>(
    app: &mut App,
    inspector: &Entity<Inspector>,
    listening: u64,
    events: Option<StreamReader<E>>,
    tell: fn(&mut Live, E),
) -> bool {
    let Some(mut events) = events else {
        return false;
    };
    let inspector = inspector.clone();
    app.spawn(async move |cx: &mut AsyncApp| {
        while let Some(event) = events.next().await {
            let wanted =
                cx.update(|app| told(app, &inspector, listening, |live| tell(live, event)));
            if !wanted {
                return;
            }
        }
    });
    true
}

/// Tells the inspector's `Live` something heard by `listening`, where that listening is still the
/// one wanted; answers whether it is.
fn told(
    app: &mut App,
    inspector: &Entity<Inspector>,
    listening: u64,
    tell: impl FnOnce(&mut Live),
) -> bool {
    inspector.update(app, |inspector, cx| {
        if inspector.live.listening != listening {
            return false;
        }
        tell(&mut inspector.live);
        cx.notify();
        true
    })
}
