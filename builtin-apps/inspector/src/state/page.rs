//! The page of the program chosen: the tab it shows, its report's table of domains, and its live
//! panels, one entity each, made for each program chosen. Only the panel shown listens, from when
//! it is shown or its program starts; showing another tab stops the panel that showed, and
//! choosing another program stops them all.
//!
//! The invariant: the page follows the program chosen: its panels are of that program and no
//! other, so a panel never hears a program after another is chosen.

use std::rc::Rc;

use waft::{App, Context, Entity, Ref, Subscription};
use waft_winui::TableViewSource;
use waos_activity::AccessKind;
use wasdk::inspect::devices::Device;
use wasdk::inspect::{active_uses, trace};

use crate::clock;
use crate::logic::panels::access::{AccessLog, AccessRow, Capturing};
use crate::logic::panels::files::{FileLog, FileRow};
use crate::logic::panels::in_use::{InUse, InUseRow};
use crate::logic::panels::network::{NetworkLog, NetworkRow};
use crate::logic::panels::output::{OutputLine, OutputLog};
use crate::logic::report::{Contacted, Report};
use crate::state::panel::{LivePanel, Log, hear};
use crate::state::programs::{OWN_ID, Programs};
use crate::words;

impl Log for NetworkLog {
    type Row = NetworkRow;
}

impl Log for FileLog {
    type Row = FileRow;
}

impl Log for AccessLog {
    type Row = AccessRow;
}

impl Log for OutputLog {
    type Row = OutputLine;
}

impl Log for InUse {
    type Row = InUseRow;
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

    /// The panel showing more of `kind` as it happens.
    pub(crate) fn of_access(kind: AccessKind) -> Tab {
        match kind {
            AccessKind::Network => Tab::Network,
            AccessKind::Files => Tab::Files,
            AccessKind::Camera
            | AccessKind::Microphone
            | AccessKind::Clipboard
            | AccessKind::Machines
            | AccessKind::Links
            | AccessKind::Notifications
            | AccessKind::Questions => Tab::Access,
        }
    }

    /// What its panel shows, as a phone's report's way to it says.
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

/// The live panels of one program; a clone is handles on the same panels.
#[derive(Clone)]
pub(crate) struct Panels {
    /// The program they are of, by its id.
    id: String,
    pub(crate) network: Entity<LivePanel<NetworkLog>>,
    pub(crate) files: Entity<LivePanel<FileLog>>,
    pub(crate) access: Entity<LivePanel<AccessLog>>,
    pub(crate) output: Entity<LivePanel<OutputLog>>,
    pub(crate) in_use: Entity<LivePanel<InUse>>,
}

pub(crate) struct ProgramPage {
    pub(crate) programs: Entity<Programs>,
    pub(crate) tab: Tab,
    panels: Option<Panels>,
    /// The domains the report of the program chosen lists, in its table.
    pub(crate) domains: Ref<TableViewSource<Contacted>>,
    _following: Subscription,
}

impl ProgramPage {
    /// The page of whichever program `programs` has chosen, from now on.
    pub(crate) fn open(app: &mut App, programs: &Entity<Programs>) -> Entity<ProgramPage> {
        let domains = TableViewSource::from(app, Vec::new());
        app.new_entity(|cx| ProgramPage {
            programs: programs.clone(),
            tab: Tab::Report,
            panels: None,
            domains,
            _following: cx.observe(programs, |page: &mut ProgramPage, _programs, cx| {
                page.follow(cx);
            }),
        })
    }

    /// The panels of the program chosen.
    pub(crate) fn panels(&self) -> Option<&Panels> {
        self.panels.as_ref()
    }

    /// The programs changed, or the one chosen: the panels follow the program chosen, its
    /// report's domains its days, and the panel shown listens where its program started.
    fn follow(&mut self, cx: &mut Context<ProgramPage>) {
        let programs = self.programs.read(cx);
        let chosen = programs.chosen(cx).map(|seen| {
            let report = Report::of(&seen.days, clock::today());
            (
                seen.program.id.clone(),
                seen.program.running,
                report.domains,
            )
        });
        let device = programs.device();
        let Some((id, running, domains)) = chosen else {
            self.leave(cx);
            cx.notify();
            return;
        };
        if self.panels.as_ref().is_none_or(|panels| panels.id != id) {
            self.leave(cx);
            self.tab = Tab::Report;
            self.panels = Some(Panels::new(cx, id.clone()));
        }
        self.show_domains(cx, domains);
        if let (true, Some(device), Some(panels)) = (running, device, &self.panels)
            && panels.listens(cx, self.tab) == Some(false)
        {
            panels.listen(cx, self.tab, &device);
        }
        cx.notify();
    }

    /// Stops the panels of the program that was chosen, which go.
    fn leave(&mut self, app: &mut App) {
        if let Some(panels) = self.panels.take() {
            for tab in Tab::ALL {
                panels.stop(app, tab);
            }
        }
    }

    fn show_domains(&self, app: &mut App, domains: Vec<Contacted>) {
        let source = self.domains.handle();
        let shown: Vec<&Contacted> = (0..source.item_count(app))
            .map(|index| source.item(app, index))
            .collect();
        if shown.len() != domains.len() || shown.iter().zip(&domains).any(|(a, b)| *a != b) {
            source.set_items(app, domains);
        }
    }
}

impl Panels {
    fn new(app: &mut App, id: String) -> Panels {
        Panels {
            id,
            network: LivePanel::new(app),
            files: LivePanel::new(app),
            access: LivePanel::new(app),
            output: LivePanel::new(app),
            in_use: LivePanel::new(app),
        }
    }

    /// Whether the panel of `tab` still hears any of its streams; none for the report.
    fn listens(&self, app: &App, tab: Tab) -> Option<bool> {
        match tab {
            Tab::Report => None,
            Tab::Network => Some(self.network.read(app).listens()),
            Tab::Files => Some(self.files.read(app).listens()),
            Tab::Access => Some(self.access.read(app).listens()),
            Tab::Output => Some(self.output.read(app).listens()),
            Tab::InUse => Some(self.in_use.read(app).listens()),
        }
    }

    /// Stops the panel of `tab`.
    fn stop(&self, app: &mut App, tab: Tab) {
        match tab {
            Tab::Report => {}
            Tab::Network => self.network.update(app, |panel, _cx| panel.stop()),
            Tab::Files => self.files.update(app, |panel, _cx| panel.stop()),
            Tab::Access => self.access.update(app, |panel, _cx| panel.stop()),
            Tab::Output => self.output.update(app, |panel, _cx| panel.stop()),
            Tab::InUse => self.in_use.update(app, |panel, _cx| panel.stop()),
        }
    }

    /// The panel of `tab` listens to its streams of the program from now on, its rows new.
    fn listen(&self, app: &mut App, tab: Tab, device: &Device) {
        let id = self.id.as_str();
        match tab {
            Tab::Report => {}
            Tab::Network => {
                let listening = self.network.update(app, |panel, cx| panel.begin(cx));
                if let Some(events) = trace::network(device, id) {
                    hear(app, &self.network, listening, events, |log, rows, event| {
                        log.add(rows, &event);
                    });
                }
            }
            Tab::Files => {
                let listening = self.files.update(app, |panel, cx| panel.begin(cx));
                if let Some(events) = trace::files(device, id) {
                    hear(app, &self.files, listening, events, |log, rows, event| {
                        log.add(rows, &event);
                    });
                }
            }
            Tab::Access => self.listen_to_access(app, device),
            Tab::Output => {
                let listening = self.output.update(app, |panel, cx| panel.begin(cx));
                if let Some(chunks) = trace::output(device, id) {
                    hear(app, &self.output, listening, chunks, |log, rows, chunk| {
                        log.add(rows, &chunk);
                    });
                }
            }
            Tab::InUse => {
                let listening = self.in_use.update(app, |panel, cx| panel.begin(cx));
                if let Some((now, changes)) = active_uses::active_uses(device, id) {
                    self.in_use.update(app, |panel, cx| {
                        panel.tell(cx, |log, rows| log.began(rows, now));
                    });
                    hear(
                        app,
                        &self.in_use,
                        listening,
                        changes,
                        |log, rows, change| {
                            log.change(rows, change);
                        },
                    );
                }
            }
        }
    }

    /// The streams of what the program reaches that its user decides, into the one panel: the
    /// camera's, the microphone's, the clipboard's, the links', the notifications', the questions'
    /// and the machines'.
    fn listen_to_access(&self, app: &mut App, device: &Device) {
        let (id, panel) = (self.id.as_str(), &self.access);
        let listening = panel.update(app, |panel, cx| panel.begin(cx));
        if let Some(events) = trace::camera(device, id) {
            hear(app, panel, listening.clone(), events, |log, rows, event| {
                log.add_capture(rows, Capturing::Camera, &event);
            });
        }
        if let Some(events) = trace::microphone(device, id) {
            hear(app, panel, listening.clone(), events, |log, rows, event| {
                log.add_capture(rows, Capturing::Microphone, &event);
            });
        }
        if let Some(events) = trace::clipboard(device, id) {
            hear(app, panel, listening.clone(), events, |log, rows, event| {
                log.add_clipboard(rows, &event);
            });
        }
        if let Some(events) = trace::links(device, id) {
            hear(app, panel, listening.clone(), events, |log, rows, event| {
                log.add_link(rows, &event);
            });
        }
        if let Some(events) = trace::notifications(device, id) {
            hear(app, panel, listening.clone(), events, |log, rows, event| {
                log.add_notification(rows, &event);
            });
        }
        if let Some(events) = trace::permissions(device, id) {
            hear(app, panel, listening.clone(), events, |log, rows, event| {
                log.add_permission(rows, &event);
            });
        }
        if let Some(events) = trace::machines(device, id) {
            hear(app, panel, listening, events, |log, rows, event| {
                log.add_machine(rows, &event);
            });
        }
    }
}

/// Shows `tab` of the program chosen's page: the panel that showed stops, and the one shown
/// listens where its program runs.
pub(crate) fn show_tab(app: &mut App, page: &Entity<ProgramPage>, tab: Tab) {
    let shown = page.update(app, |page, cx| {
        let shown = page.tab;
        page.tab = tab;
        cx.notify();
        shown
    });
    let Some((device, running)) = chosen_device(app, page) else {
        return;
    };
    let Some(panels) = page.read(app).panels.clone() else {
        return;
    };
    panels.stop(app, shown);
    if running {
        panels.listen(app, tab, &device);
    }
}

/// Starts the program chosen again, or starts it where it ended, held until the panel shown
/// listens to it, so the panel hears it from its first instruction (decision 18 of
/// `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`); one WaOS could not start says
/// why in the panel.
pub(crate) fn start_inspected(app: &mut App, page: &Entity<ProgramPage>) {
    let Some((device, _running)) = chosen_device(app, page) else {
        return;
    };
    let read = page.read(app);
    let tab = read.tab;
    let Some(panels) = read.panels.clone().filter(|panels| panels.id != OWN_ID) else {
        return;
    };
    panels.stop(app, tab);
    app.spawn(async move |cx| {
        let started = device.start_held(panels.id.clone()).await;
        cx.update(|app| match started {
            // Listened to before it is let go, as the held program is dropped.
            Ok(_held) => panels.listen(app, tab, &device),
            Err(error) => panels.refused(app, tab, words::start_refused(&error)),
        });
    });
}

/// Ends the program chosen, with a stop, as its user may from the inspector (decision 19); its
/// panels keep what they heard.
pub(crate) fn end_program(app: &mut App, page: &Entity<ProgramPage>) {
    let Some((device, _running)) = chosen_device(app, page) else {
        return;
    };
    let Some(id) = page
        .read(app)
        .panels
        .as_ref()
        .map(|panels| panels.id.clone())
    else {
        return;
    };
    app.spawn(async move |_cx| device.end(id).await);
}

/// The device, and whether the program chosen runs, where one is chosen.
fn chosen_device(app: &App, page: &Entity<ProgramPage>) -> Option<(Rc<Device>, bool)> {
    let programs = page.read(app).programs.read(app);
    let running = programs.chosen(app)?.program.running;
    Some((programs.device()?, running))
}

impl Panels {
    /// WaOS did not start the program for the panel of `tab`, for `why`.
    fn refused(&self, app: &mut App, tab: Tab, why: String) {
        match tab {
            Tab::Report => {}
            Tab::Network => self.network.update(app, |panel, cx| panel.refused(why, cx)),
            Tab::Files => self.files.update(app, |panel, cx| panel.refused(why, cx)),
            Tab::Access => self.access.update(app, |panel, cx| panel.refused(why, cx)),
            Tab::Output => self.output.update(app, |panel, cx| panel.refused(why, cx)),
            Tab::InUse => self.in_use.update(app, |panel, cx| panel.refused(why, cx)),
        }
    }
}
