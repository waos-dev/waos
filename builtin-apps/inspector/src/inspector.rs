//! The app's state: the device inspected, this one, and the programs on it, each with what it did
//! over the days kept, those that used something guarded lately first; the program chosen; and the
//! icons asked for so far. The programs are read when the app starts and again at each change the
//! device tells, each program's icon only as its row is first shown (decision 10 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`).
//!
//! The invariant: every read of the device is a task of its own, which updates the state once it
//! has its answer, so the window never waits on the device.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use waft::{App, Context, Entity};
use waos_activity::Activity;
use wasdk::inspect::audit;
use wasdk::inspect::devices::{self, Device, Program};

use crate::activity::activity_of;

/// What the app knows of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Found {
    /// Its programs are being read.
    Looking,
    /// WaOS shows this program no device: it does not hold the inspector role.
    NoDevice,
    Ready,
}

/// A program on the device, and what it did over the days kept.
#[derive(Clone, Debug)]
pub(crate) struct Seen {
    pub(crate) program: Program,
    pub(crate) activity: Activity,
}

/// An icon, as far as it is known.
enum Icon {
    Asked,
    Got(Option<Arc<[u8]>>),
}

pub(crate) struct Inspector {
    device: Option<Rc<Device>>,
    pub(crate) device_name: String,
    pub(crate) found: Found,
    /// The programs, those used most lately first, the rest by name.
    pub(crate) programs: Vec<Seen>,
    /// The program whose activity shows, by its id.
    pub(crate) chosen: Option<String>,
    icons: HashMap<String, Icon>,
}

impl Inspector {
    /// The app's state, its programs read from the device from now on.
    pub(crate) fn open(app: &mut App) -> Entity<Inspector> {
        let inspector = app.new_entity(|_cx| Inspector {
            device: None,
            device_name: String::new(),
            found: Found::Looking,
            programs: Vec::new(),
            chosen: None,
            icons: HashMap::new(),
        });
        find_device(app, &inspector);
        inspector
    }

    /// The program chosen, where it is still on the device.
    pub(crate) fn chosen(&self) -> Option<&Seen> {
        let chosen = self.chosen.as_deref()?;
        self.programs.iter().find(|seen| seen.program.id == chosen)
    }

    /// Shows the activity of the program `id`, or the list of programs alone with none.
    pub(crate) fn choose(&mut self, id: Option<String>, cx: &mut Context<Inspector>) {
        self.chosen = id;
        cx.notify();
    }

    /// The icon of the program `id`, a PNG; none until it is read, or for one with none.
    pub(crate) fn icon(&self, id: &str) -> Option<Arc<[u8]>> {
        match self.icons.get(id) {
            Some(Icon::Got(png)) => png.clone(),
            _ => None,
        }
    }

    fn set_programs(&mut self, mut programs: Vec<Seen>, cx: &mut Context<Inspector>) {
        programs.sort_by(|left, right| {
            let lately = right.activity.last_use.cmp(&left.activity.last_use);
            lately.then_with(|| left.program.name.cmp(&right.program.name))
        });
        self.programs = programs;
        self.found = Found::Ready;
        cx.notify();
    }
}

/// Asks for the icon of the program `id` where it was not asked for yet, and shows it once read.
pub(crate) fn want_icon(app: &mut App, inspector: &Entity<Inspector>, id: &str) {
    let device = inspector.update(app, |inspector, _cx| {
        if inspector.icons.contains_key(id) {
            return None;
        }
        inspector.icons.insert(id.to_owned(), Icon::Asked);
        inspector.device.clone()
    });
    let Some(device) = device else {
        return;
    };
    let (inspector, id) = (inspector.clone(), id.to_owned());
    app.spawn(async move |cx| {
        let png = device.icon(id.clone()).await.map(Arc::from);
        cx.update(|app| {
            inspector.update(app, |inspector, cx| {
                inspector.icons.insert(id, Icon::Got(png));
                cx.notify();
            });
        });
    });
}

/// Finds this device, reads its programs, and reads them again at each change it tells.
fn find_device(app: &mut App, inspector: &Entity<Inspector>) {
    let inspector = inspector.clone();
    app.spawn(async move |cx| {
        let Some(device) = devices::devices().into_iter().next() else {
            cx.update(|app| {
                inspector.update(app, |inspector, cx| {
                    inspector.found = Found::NoDevice;
                    cx.notify();
                });
            });
            return;
        };
        let device = Rc::new(device);
        let mut changes = device.program_changes();
        cx.update(|app| {
            inspector.update(app, |inspector, _cx| {
                inspector.device_name = device.name();
                inspector.device = Some(Rc::clone(&device));
            });
        });
        loop {
            let programs = read_programs(&device).await;
            cx.update(|app| {
                inspector.update(app, |inspector, cx| inspector.set_programs(programs, cx));
            });
            if changes.next().await.is_none() {
                return;
            }
        }
    });
}

/// Each program on `device`, with what it did over the days kept.
async fn read_programs(device: &Device) -> Vec<Seen> {
    let mut seen = Vec::new();
    for program in device.programs().await {
        let days = audit::days(device, program.id.clone()).await;
        seen.push(Seen {
            activity: activity_of(&days),
            program,
        });
    }
    seen
}
