//! The app's state: the device inspected, this one, and the programs on it, each with what it did
//! over the days kept, those that used something guarded lately first; the program chosen, the
//! tab of its page shown, and what that tab's panel hears as it happens; and the icons asked for
//! so far. The programs are read when the app starts and again at each change the
//! device tells, each program's icon only as its row is first shown (decision 10 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`).
//!
//! The invariant: every read of the device is a task of its own, which updates the state once it
//! has its answer, so the window never waits on the device.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use waft::ui::{Image, Size};
use waft::{App, AsyncApp, Context, Entity};
use waos_activity::Activity;
use wasdk::inspect::audit;
use wasdk::inspect::devices::{self, Device, Program};

use crate::activity::activity_of;
use crate::live::{self, Live};
use crate::words;

/// The inspector's own id, as its package names it: it never starts itself.
pub(crate) const OWN_ID: &str = "dev.waos.inspector";

/// The icon a program with no picture of its own shows, WaOS's default.
const DEFAULT_PNG: &[u8] = include_bytes!("../assets/program-default.png");

/// The side, in pixels, icons are decoded at: the largest drawn, at twice its points.
const ICON_PIXELS: f64 = 96.0;

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

/// The tabs of a program's page: its report, then the live panels (decision 14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Report,
    Network,
    Files,
    Access,
    Output,
    InUse,
}

impl Tab {
    pub(crate) const ALL: [Tab; 6] = [
        Tab::Report,
        Tab::Network,
        Tab::Files,
        Tab::Access,
        Tab::Output,
        Tab::InUse,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tab::Report => words::REPORT,
            Tab::Network => words::NETWORK,
            Tab::Files => words::FILES,
            Tab::Access => words::ACCESS_TAB,
            Tab::Output => words::OUTPUT,
            Tab::InUse => words::IN_USE,
        }
    }

    /// The panel showing more of an access the report tells, by the access's name; none for one
    /// no panel shows.
    pub(crate) fn of_access(kind: &str) -> Option<Tab> {
        match kind {
            "Files" => Some(Tab::Files),
            "Camera" | "Microphone" | "Clipboard" | "Machines" | "Notifications" => {
                Some(Tab::Access)
            }
            _ => None,
        }
    }

    /// What its panel shows, as the report's way to it says.
    pub(crate) fn shows(self) -> &'static str {
        match self {
            Tab::Report => words::REPORT,
            Tab::Network => words::SEE_NETWORK,
            Tab::Files => words::SEE_FILES,
            Tab::Access => words::SEE_ACCESS,
            Tab::Output => words::SEE_OUTPUT,
            Tab::InUse => words::SEE_IN_USE,
        }
    }
}

/// An icon, as far as it is known: asked for, or read and decoded by the system's codecs; none
/// where the system could not decode it.
enum Icon {
    Asked,
    Got(Option<Image>),
}

pub(crate) struct Inspector {
    device: Option<Rc<Device>>,
    pub(crate) device_name: String,
    pub(crate) found: Found,
    /// The programs, those used most lately first, the rest by name.
    pub(crate) programs: Vec<Seen>,
    /// The program whose activity shows, by its id.
    pub(crate) chosen: Option<String>,
    /// The tab of its page that shows.
    pub(crate) tab: Tab,
    /// What the panel shown hears.
    pub(crate) live: Live,
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
            tab: Tab::Report,
            live: Live::default(),
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

    /// Shows the report of the program `id`, or the list of programs alone with none.
    pub(crate) fn choose(&mut self, id: Option<String>, cx: &mut Context<Inspector>) {
        if self.chosen != id {
            self.tab = Tab::Report;
            self.live.stop();
        }
        self.chosen = id;
        cx.notify();
    }

    /// The icon of the program `id`, decoded; none until it is read and decoded, or where the
    /// system could not decode it.
    pub(crate) fn icon(&self, id: &str) -> Option<Image> {
        match self.icons.get(id) {
            Some(Icon::Got(image)) => image.clone(),
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

/// Shows `tab` of the program chosen's page, listening to what its panel shows from now on.
pub(crate) fn show_tab(app: &mut App, inspector: &Entity<Inspector>, tab: Tab) {
    let wanted = inspector.update(app, |inspector, cx| {
        inspector.tab = tab;
        let listening = inspector.live.stop();
        cx.notify();
        Some((
            inspector.device.clone()?,
            inspector.chosen.clone()?,
            listening,
        ))
    });
    if let Some((device, id, listening)) = wanted {
        live::listen(app, inspector, (&device, &id), tab, listening);
    }
}

/// Starts the program chosen again, or starts it where it ended, held until the panel shown
/// listens to it, so the panel hears it from its first instruction (decision 18 of
/// `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`); one WaOS could not start says
/// why.
pub(crate) fn start_inspected(app: &mut App, inspector: &Entity<Inspector>) {
    let wanted = inspector.update(app, |inspector, cx| {
        let listening = inspector.live.stop();
        cx.notify();
        Some((
            inspector.device.clone()?,
            inspector.chosen.clone()?,
            inspector.tab,
            listening,
        ))
    });
    let Some((device, id, tab, listening)) = wanted else {
        return;
    };
    let inspector = inspector.clone();
    app.spawn(async move |cx| {
        let started = device.start_held(id.clone()).await;
        cx.update(|app| match started {
            // Listened to before it is let go, as the held program is dropped.
            Ok(_held) => live::listen(app, &inspector, (&device, &id), tab, listening),
            Err(error) => inspector.update(app, |inspector, cx| {
                inspector
                    .live
                    .start_refused(listening, words::start_refused(&error));
                cx.notify();
            }),
        });
    });
}

/// Ends the program chosen, with a stop, as its user may from the inspector (decision 19); its
/// panel keeps what it heard.
pub(crate) fn end_program(app: &mut App, inspector: &Entity<Inspector>) {
    let read = inspector.read(app);
    let (Some(device), Some(id)) = (read.device.clone(), read.chosen.clone()) else {
        return;
    };
    app.spawn(async move |_cx| device.end(id).await);
}

/// Starts hearing what the panel shown shows where nothing was heard, as when the program did not
/// run as the panel was shown and runs now.
fn listen_if_it_started(app: &mut App, inspector: &Entity<Inspector>) {
    let wanted = {
        let read = inspector.read(app);
        let running = read.chosen().is_some_and(|seen| seen.program.running);
        let unheard = read.tab != Tab::Report && !read.live.heard && running;
        unheard.then(|| Some((read.device.clone()?, read.chosen.clone()?, read.tab)))
    };
    if let Some(Some((device, id, tab))) = wanted {
        let listening = inspector.update(app, |inspector, _cx| inspector.live.stop());
        live::listen(app, inspector, (&device, &id), tab, listening);
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
        let png: Arc<[u8]> = match device.icon(id.clone()).await {
            Some(png) => Arc::from(png),
            None => Arc::from(DEFAULT_PNG),
        };
        let image = decode(cx, png).await;
        cx.update(|app| {
            inspector.update(app, |inspector, cx| {
                inspector.icons.insert(id, Icon::Got(image));
                cx.notify();
            });
        });
    });
}

/// `png` decoded by the system's codecs at the size the icons are drawn at; none where the system
/// has no codecs or does not read it.
async fn decode(cx: &AsyncApp, png: Arc<[u8]>) -> Option<Image> {
    let opening = cx.update(|app| Some(app.platform().image_codecs()?.open(png)))?;
    let source = opening.await.ok()?;
    let pixels = source
        .decode_pixels(0, Some(Size::new(ICON_PIXELS, ICON_PIXELS)))
        .await
        .ok()?;
    cx.update(|app| app.platform().import_pixels(pixels))
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
                listen_if_it_started(app, &inspector);
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
            days,
        });
    }
    seen
}
