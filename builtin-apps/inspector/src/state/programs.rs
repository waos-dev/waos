//! The device inspected, this one, and the programs on it, each with what it did over the days
//! kept, those that used something guarded lately first; which of them is chosen; and each one's
//! icon, read only as its row is first shown. The programs are read when the app starts and again
//! at each change the device tells.
//!
//! The program chosen is the selection of the list's `ItemsViewSource`, which this entity owns, as
//! a WinUI app owns its `ItemsSource`: the user's click, a phone's tap and the way back all change
//! that selection, and every change of it notifies the entity.
//!
//! The invariant: the source is only ever changed outside this entity's update, since each change
//! of it notifies the entity; and every read of the device is a task of its own, which updates the
//! state once it has its answer, so the window never waits on the device.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use waft::{App, Entity, ListenableObject, Listener, Ref};
use waft_winui::ItemsViewSource;
use waos_activity::Activity;
use wasdk::inspect::audit;
use wasdk::inspect::devices::{self, Device, Program};

use crate::logic::activity::activity_of;

/// The inspector's own id, as its package names it: it never starts itself.
pub(crate) const OWN_ID: &str = "dev.waos.inspector";

/// The icon a program with no picture of its own shows, WaOS's default.
const DEFAULT_PNG: &[u8] = include_bytes!("../../assets/program-default.png");

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
    /// The days kept of it, newest first.
    pub(crate) days: Vec<audit::Day>,
}

/// An icon, as far as it is known: asked for, or its PNG.
enum Icon {
    Asked,
    Got(Arc<[u8]>),
}

pub(crate) struct Programs {
    device: Option<Rc<Device>>,
    pub(crate) device_name: String,
    pub(crate) found: Found,
    /// The programs, those used most lately first, the rest by name.
    pub(crate) list: Vec<Seen>,
    /// The list's items and its selection, the program chosen.
    selection: Ref<ItemsViewSource>,
    icons: HashMap<String, Icon>,
}

impl Programs {
    /// The programs, read from the device from now on, none chosen.
    pub(crate) fn open(app: &mut App) -> Entity<Programs> {
        let selection = ItemsViewSource::new(app, 0);
        let programs = app.new_entity(|_cx| Programs {
            device: None,
            device_name: String::new(),
            found: Found::Looking,
            list: Vec::new(),
            selection: selection.clone(),
            icons: HashMap::new(),
        });
        let chosen = programs.downgrade();
        selection.handle().add_listener(
            app,
            Listener::new(move |app| {
                if let Some(programs) = chosen.upgrade() {
                    programs.update(app, |_programs, cx| cx.notify());
                }
            }),
        );
        find_device(app, &programs);
        programs
    }

    /// The list's items and selection, for the list to show.
    pub(crate) fn selection(&self) -> Ref<ItemsViewSource> {
        self.selection.clone()
    }

    pub(crate) fn device(&self) -> Option<Rc<Device>> {
        self.device.clone()
    }

    /// The program chosen.
    pub(crate) fn chosen<'a>(&'a self, app: &App) -> Option<&'a Seen> {
        let index = self.selection.handle().selected_index(app)?;
        self.list.get(index)
    }

    /// An empty list for a test: no device, nothing chosen.
    #[cfg(test)]
    pub(crate) fn none(app: &mut App) -> Entity<Programs> {
        let selection = ItemsViewSource::new(app, 0);
        app.new_entity(|_cx| Programs {
            device: None,
            device_name: String::new(),
            found: Found::NoDevice,
            list: Vec::new(),
            selection,
            icons: HashMap::new(),
        })
    }

    /// The icon of the program `id`, as a PNG; none until it is read.
    pub(crate) fn icon(&self, id: &str) -> Option<Arc<[u8]>> {
        match self.icons.get(id) {
            Some(Icon::Got(png)) => Some(Arc::clone(png)),
            _ => None,
        }
    }
}

/// Chooses the program at `index` of the list, or none, as a tap on a phone's tile or its way
/// back does.
pub(crate) fn choose(app: &mut App, programs: &Entity<Programs>, index: Option<usize>) {
    let selection = programs.read(app).selection.handle();
    match index {
        Some(index) => selection.select(app, index),
        None => selection.deselect_all(app),
    }
}

/// Asks for the icon of the program `id` where it was not asked for yet, and shows it once read.
pub(crate) fn want_icon(app: &mut App, programs: &Entity<Programs>, id: &str) {
    let device = programs.update(app, |programs, _cx| {
        if programs.icons.contains_key(id) {
            return None;
        }
        programs.icons.insert(id.to_owned(), Icon::Asked);
        programs.device.clone()
    });
    let Some(device) = device else {
        return;
    };
    let (programs, id) = (programs.clone(), id.to_owned());
    app.spawn(async move |cx| {
        let png: Arc<[u8]> = match device.icon(id.clone()).await {
            Some(png) => Arc::from(png),
            None => Arc::from(DEFAULT_PNG),
        };
        cx.update(|app| {
            programs.update(app, |programs, cx| {
                programs.icons.insert(id, Icon::Got(png));
                cx.notify();
            });
        });
    });
}

/// Gives the programs `list`, those used most lately first; the program chosen stays chosen
/// wherever it moved, and none is chosen once it is gone.
fn set_list(app: &mut App, programs: &Entity<Programs>, mut list: Vec<Seen>) {
    list.sort_by(|left, right| {
        let lately = right.activity.last_use.cmp(&left.activity.last_use);
        lately.then_with(|| left.program.name.cmp(&right.program.name))
    });
    let (was_chosen, listed, selection) = {
        let read = programs.read(app);
        let chosen = read.chosen(app).map(|seen| seen.program.id.clone());
        let listed: Vec<String> = read
            .list
            .iter()
            .map(|seen| seen.program.id.clone())
            .collect();
        (chosen, listed, read.selection.handle())
    };
    let ids: Vec<String> = list.iter().map(|seen| seen.program.id.clone()).collect();
    programs.update(app, |programs, cx| {
        programs.list = list;
        programs.found = Found::Ready;
        cx.notify();
    });
    if ids != listed {
        selection.set_items(app, ids.len());
        let at = was_chosen.and_then(|chosen| ids.iter().position(|id| *id == chosen));
        if let Some(index) = at {
            selection.select(app, index);
        }
    }
}

/// Finds this device, reads its programs, and reads them again at each change it tells.
fn find_device(app: &mut App, programs: &Entity<Programs>) {
    let programs = programs.clone();
    app.spawn(async move |cx| {
        let Some(device) = devices::devices().into_iter().next() else {
            cx.update(|app| {
                programs.update(app, |programs, cx| {
                    programs.found = Found::NoDevice;
                    cx.notify();
                });
            });
            return;
        };
        let device = Rc::new(device);
        let mut changes = device.program_changes();
        cx.update(|app| {
            programs.update(app, |programs, _cx| {
                programs.device_name = device.name();
                programs.device = Some(Rc::clone(&device));
            });
        });
        loop {
            let list = read_programs(&device).await;
            cx.update(|app| set_list(app, &programs, list));
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
            days,
        });
    }
    seen
}
