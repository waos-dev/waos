//! The page of the program chosen: the tab it shows, its report's table of domains, its live
//! panels, one entity each, made for each program chosen, and its storage. Every live panel
//! listens while a program is chosen and runs, from when it is chosen or from its start, and keeps
//! its rows across tab switches and rebuilds; choosing another program stops them all, and a
//! program ending keeps their rows, as it does now.
//!
//! The invariant: the page follows the program chosen: its panels are of that program and no
//! other, so a panel never hears a program after another is chosen; and a tab shows what its
//! panel holds, which a tab switch never changes.

use std::rc::Rc;

use waft::{App, Context, Entity, Ref, Subscription};
use waft_winui::TableViewSource;
use waos_activity::AccessKind;
use wasdk::inspect::devices::Device;
use wasdk::inspect::{active_uses, storage as inspect_storage, trace};

use crate::clock;
use crate::logic::panels::PanelRow;
use crate::logic::panels::access::{AccessLog, AccessRow, Capturing};
use crate::logic::panels::files::{FileLog, FileRow};
use crate::logic::panels::in_use::{InUse, InUseRow};
use crate::logic::panels::network::{NetworkLog, NetworkRow};
use crate::logic::panels::output::{OutputLine, OutputLog};
use crate::logic::report::{Contacted, Report};
use crate::state::panel::{LivePanel, Log, hear, stop};
use crate::state::programs::{OWN_ID, Programs};
use crate::state::storage::Storage;
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
    Storage,
    Access,
    Output,
    InUse,
}

impl Tab {
    pub(crate) const ALL: [Tab; 7] = [
        Tab::Report,
        Tab::Network,
        Tab::Files,
        Tab::Storage,
        Tab::Access,
        Tab::Output,
        Tab::InUse,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tab::Report => words::REPORT,
            Tab::Network => words::NETWORK,
            Tab::Files => words::FILES,
            Tab::Storage => words::STORAGE,
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
            Tab::Storage => words::SEE_STORAGE,
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
    /// Whether the panels listen to the program chosen now.
    listening: bool,
    /// Whether a start inspected is held, waiting for every panel to listen.
    starting: bool,
    /// The domains the report of the program chosen lists, in its table.
    pub(crate) domains: Ref<TableViewSource<Contacted>>,
    /// The storage tab's mounts and the folders opened of the program chosen.
    storage: Storage,
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
            listening: false,
            starting: false,
            domains,
            storage: Storage::default(),
            _following: cx.observe(programs, |page: &mut ProgramPage, _programs, cx| {
                page.follow(cx);
            }),
        })
    }

    /// The panels of the program chosen.
    pub(crate) fn panels(&self) -> Option<&Panels> {
        self.panels.as_ref()
    }

    /// The storage of the program chosen.
    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    /// The programs changed, or the one chosen: the panels follow the program chosen, its
    /// report's domains its days, and every panel listens where its program runs.
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
        let chosen_new = self.panels.as_ref().is_none_or(|panels| panels.id != id);
        if chosen_new {
            self.leave(cx);
            self.tab = Tab::Report;
            self.panels = Some(Panels::new(cx, id.clone()));
            self.listening = false;
            self.starting = false;
            self.storage.reset();
            if let Some(device) = &device {
                read_mounts(cx, device, &id);
            }
        }
        self.show_domains(cx, domains);
        if running {
            if let (Some(device), Some(panels)) = (device, &self.panels)
                && !self.starting
                && !self.listening
            {
                panels.listen_all(cx, &device);
                self.listening = true;
            }
        } else {
            self.listening = false;
        }
        cx.notify();
    }

    /// Stops the panels of the program that was chosen, which go.
    fn leave(&mut self, app: &mut App) {
        if let Some(panels) = self.panels.take() {
            panels.stop_all(app);
        }
        self.listening = false;
        self.starting = false;
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

    /// Stops every panel, which goes out of sight with its program.
    fn stop_all(&self, app: &mut App) {
        stop(app, &self.network);
        stop(app, &self.files);
        stop(app, &self.access);
        stop(app, &self.output);
        stop(app, &self.in_use);
    }

    /// Every panel listens to its streams of the program from now on, its rows new.
    fn listen_all(&self, app: &mut App, device: &Device) {
        for tab in [
            Tab::Network,
            Tab::Files,
            Tab::Access,
            Tab::Output,
            Tab::InUse,
        ] {
            self.listen(app, tab, device);
        }
    }

    /// How many rows the panel of `tab` holds, and how many of them WaOS refused; none for the
    /// report, which keeps no rows.
    pub(crate) fn counted(&self, app: &App, tab: Tab) -> Option<(usize, usize)> {
        fn count<L: Log>(app: &App, panel: &Entity<LivePanel<L>>) -> (usize, usize) {
            let read = panel.read(app);
            let rows: Vec<&L::Row> = read.all(app).collect();
            (rows.len(), rows.iter().filter(|row| row.refused()).count())
        }
        match tab {
            Tab::Report | Tab::Storage => None,
            Tab::Network => Some(count(app, &self.network)),
            Tab::Files => Some(count(app, &self.files)),
            Tab::Access => Some(count(app, &self.access)),
            Tab::Output => Some(count(app, &self.output)),
            Tab::InUse => Some(count(app, &self.in_use)),
        }
    }

    /// The panel of `tab` listens to its streams of the program from now on, its rows new.
    fn listen(&self, app: &mut App, tab: Tab, device: &Device) {
        let id = self.id.as_str();
        match tab {
            Tab::Report | Tab::Storage => {}
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

/// Shows `tab` of the program chosen's page. Every panel listens while the program is chosen, so
/// showing another only changes what is seen.
pub(crate) fn show_tab(app: &mut App, page: &Entity<ProgramPage>, tab: Tab) {
    page.update(app, |page, cx| {
        page.tab = tab;
        cx.notify();
    });
}

/// Starts the program chosen again, or starts it where it ended, held until every panel listens
/// to it, so each hears it from its first instruction (decision 18 of
/// `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`); one WaOS could not start
/// says why in each panel.
pub(crate) fn start_inspected(app: &mut App, page: &Entity<ProgramPage>) {
    let Some((device, _running)) = chosen_device(app, page) else {
        return;
    };
    let panels = page
        .read(app)
        .panels
        .clone()
        .filter(|panels| panels.id != OWN_ID);
    let Some(panels) = panels else {
        return;
    };
    page.update(app, |page, cx| {
        page.starting = true;
        page.listening = false;
        cx.notify();
    });
    panels.stop_all(app);
    let page = page.clone();
    app.spawn(async move |cx| {
        let started = device.start_held(panels.id.clone()).await;
        cx.update(|app| {
            // The page may show another program by now, whose panels are the ones that listen.
            let showing = page
                .read(app)
                .panels()
                .is_some_and(|current| current.id == panels.id);
            if !showing {
                panels.stop_all(app);
                return;
            }
            match started {
                // Listened to before it is let go, as the held program is dropped.
                Ok(_held) => {
                    panels.listen_all(app, &device);
                    page.update(app, |page, cx| {
                        page.starting = false;
                        page.listening = true;
                        cx.notify();
                    });
                }
                Err(error) => {
                    panels.refused_all(app, words::start_refused(&error));
                    page.update(app, |page, cx| {
                        page.starting = false;
                        page.listening = false;
                        cx.notify();
                    });
                }
            }
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
    /// WaOS did not start the program, for `why`, for every panel.
    fn refused_all(&self, app: &mut App, why: String) {
        self.network
            .update(app, |panel, cx| panel.refused(why.clone(), cx));
        self.files
            .update(app, |panel, cx| panel.refused(why.clone(), cx));
        self.access
            .update(app, |panel, cx| panel.refused(why.clone(), cx));
        self.output
            .update(app, |panel, cx| panel.refused(why.clone(), cx));
        self.in_use.update(app, |panel, cx| panel.refused(why, cx));
    }
}

/// Reads the mounts of the program `id` on `device`, for the page chosen.
fn read_mounts(cx: &mut Context<ProgramPage>, device: &Rc<Device>, id: &str) {
    let page = cx.entity();
    let (device, id) = (Rc::clone(device), id.to_owned());
    cx.spawn(async move |cx| {
        let mounts = inspect_storage::mounts(&device, &id);
        cx.update(|app| {
            page.update(app, |page, cx| {
                page.storage.mounts_arrived(mounts);
                cx.notify();
            });
        });
    });
}

/// Opens the storage folder `path` of the program chosen, reading its entries the first time.
pub(crate) fn open_storage_folder(app: &mut App, page: &Entity<ProgramPage>, path: &str) {
    let read = page.update(app, |page, cx| {
        let read = page.storage.expand(path);
        if read {
            page.storage.folder_looking(path);
        }
        cx.notify();
        read
    });
    if !read {
        return;
    }
    let Some((device, _running)) = chosen_device(app, page) else {
        fail_folder(app, page, path, "It is not on this device.".to_owned());
        return;
    };
    let Some(id) = page.read(app).panels().map(|panels| panels.id.clone()) else {
        fail_folder(app, page, path, "It is not on this device.".to_owned());
        return;
    };
    let page = page.clone();
    let path = path.to_owned();
    app.spawn(async move |cx| {
        let done = inspect_storage::read_folder(&device, id, path.clone()).await;
        cx.update(|app| {
            page.update(app, |page, cx| {
                match done {
                    Ok(entries) => page.storage.folder_arrived(&path, entries),
                    Err(why) => page.storage.folder_failed(&path, why),
                }
                cx.notify();
            });
        });
    });
}

/// Closes the storage folder `path` of the program chosen; its entries are kept.
pub(crate) fn close_storage_folder(app: &mut App, page: &Entity<ProgramPage>, path: &str) {
    page.update(app, |page, cx| {
        page.storage.collapse(path);
        cx.notify();
    });
}

/// Says why the entries of `path` were not read.
fn fail_folder(app: &mut App, page: &Entity<ProgramPage>, path: &str, why: String) {
    page.update(app, |page, cx| {
        page.storage.folder_failed(path, why);
        cx.notify();
    });
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use waft::AppCell;
    use waft_test::TestPlatform;
    use wasdk::inspect::active_uses::{ActiveUse, ActiveUseKind, Capture};
    use wasdk::inspect::trace::{
        Cause, DeviceEvent, DeviceOpened, DeviceUse, FileEvent, FileUse, NameLookup, NetworkEvent,
        NetworkUse, OutputChunk, OutputStream, Verdict,
    };
    use wasdk::system_clock::Instant;

    use super::*;
    use crate::state::programs::Programs;

    #[test]
    fn every_panels_rows_and_badge_stay_as_tabs_change() {
        let cell = AppCell::with_platform(Rc::new(TestPlatform::new()));
        let mut app = cell.borrow_mut();
        let programs = Programs::none(&mut app);
        let page = ProgramPage::open(&mut app, &programs);
        let panels = Panels::new(&mut app, "dev.example.storage".to_owned());
        let at = Instant {
            seconds: 10,
            nanoseconds: 0,
        };

        panels.network.update(&mut app, |panel, cx| {
            panel.tell(cx, |log, rows| {
                log.add(
                    rows,
                    &NetworkEvent {
                        at,
                        instance: 0,
                        dropped_before: 0,
                        kind: NetworkUse::NameLookup(NameLookup {
                            name: "example.com".to_owned(),
                            cause: Cause::Program,
                            addresses: vec!["93.184.216.34".to_owned()],
                        }),
                        verdict: Verdict::Refused("network".to_owned()),
                    },
                );
            });
        });
        panels.files.update(&mut app, |panel, cx| {
            panel.tell(cx, |log, rows| {
                log.add(
                    rows,
                    &FileEvent {
                        at,
                        instance: 0,
                        dropped_before: 0,
                        path: Some("/data/a".to_owned()),
                        on_machine: None,
                        mount: None,
                        cause: Cause::Program,
                        kind: FileUse::OpenToRead,
                        verdict: Verdict::Allowed,
                    },
                );
            });
        });
        panels.access.update(&mut app, |panel, cx| {
            panel.tell(cx, |log, rows| {
                log.add_capture(
                    rows,
                    Capturing::Camera,
                    &DeviceEvent {
                        at,
                        instance: 0,
                        dropped_before: 0,
                        kind: DeviceUse::Opened(DeviceOpened {
                            camera_id: None,
                            simulated: true,
                        }),
                        verdict: Verdict::Allowed,
                    },
                );
            });
        });
        panels.output.update(&mut app, |panel, cx| {
            panel.tell(cx, |log, rows| {
                log.add(
                    rows,
                    &OutputChunk {
                        at,
                        instance: 0,
                        dropped_before: 0,
                        channel: OutputStream::Stdout,
                        bytes: b"hello\n".to_vec(),
                    },
                );
            });
        });
        panels.in_use.update(&mut app, |panel, cx| {
            panel.tell(cx, |log, rows| {
                log.began(
                    rows,
                    vec![ActiveUse {
                        id: 1,
                        kind: ActiveUseKind::Camera(Capture { simulated: true }),
                        since: at,
                    }],
                );
            });
        });
        page.update(&mut app, |page, cx| {
            page.panels = Some(panels.clone());
            cx.notify();
        });

        let counts = |app: &App| {
            [
                panels.counted(app, Tab::Network),
                panels.counted(app, Tab::Files),
                panels.counted(app, Tab::Access),
                panels.counted(app, Tab::Output),
                panels.counted(app, Tab::InUse),
            ]
        };
        assert_eq!(
            counts(&app),
            [
                Some((1, 1)),
                Some((1, 0)),
                Some((1, 0)),
                Some((1, 0)),
                Some((1, 0)),
            ],
            "a badge is its panel's rows, refused ones counted"
        );
        assert_eq!(panels.counted(&app, Tab::Report), None);
        assert_eq!(panels.counted(&app, Tab::Storage), None);

        for tab in Tab::ALL {
            show_tab(&mut app, &page, tab);
            assert_eq!(
                counts(&app),
                [
                    Some((1, 1)),
                    Some((1, 0)),
                    Some((1, 0)),
                    Some((1, 0)),
                    Some((1, 0)),
                ],
                "showing {tab:?} keeps every panel's rows"
            );
        }
    }
}
