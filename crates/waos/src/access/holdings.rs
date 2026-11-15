//! What a program holds now: each use that lasts, from the moment what owns the real thing starts
//! it until that thing really ends (a capture session, the microphone recording, a command until
//! it exits, a request until its response's body ends, a watch), and each file it has open. The
//! shell's indicator of who records reads the same record: it hears a program records with a kind
//! of device as its first one starts, and that it stopped as its last ends.
//!
//! A use held is a lock and a small entry beside work far slower; an open file is a weak
//! reference to the handle, its path asked of the system only when the record is read
//! (`../experiments/2026-11-15-audit-cost`: neither shows beside an open).
//!
//! The invariant: an entry goes exactly when its [`Held`] is dropped, wherever that is, the
//! program ending among the ways, so the record and the indicator never outlast what they show.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Instant, SystemTime};

use super::AccessParts;
use crate::files::{MountKind, OpenMode};
use crate::running::RunningProgram;
use crate::shell::{RecordingDevice, ShellRecordingIndicator};
use crate::sync::locked;

/// A use the program holds.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "what is held is read by the inspector's live state, a later step"
)]
pub(crate) enum Holding {
    /// A capture session or the microphone recording, as the indicator shows it.
    Recording(RecordingDevice),
    /// A command running on a machine.
    Run {
        machine: String,
        program: String,
        in_terminal: bool,
    },
    /// An HTTP request in flight, until its response's body ends.
    Request {
        method: http::Method,
        uri: http::Uri,
    },
    /// A folder watched, where it was on the machine when the watch began.
    Watch { folder: PathBuf, recursive: bool },
}

/// Who shows a program's recordings: the shell's indicator, which names the program.
pub(crate) struct ShownRecordings {
    pub(crate) indicator: Arc<dyn ShellRecordingIndicator>,
    pub(crate) program: RunningProgram,
}

/// A use held from its start until this is dropped.
pub(crate) struct Held {
    parts: Arc<AccessParts>,
    id: u64,
    /// The program's instance that holds it, which its end is told in.
    instance: u32,
}

/// One program's record of what it holds.
pub(super) struct Registry {
    held: Mutex<HeldUses>,
    open_files: Mutex<Vec<OpenFile>>,
    shown: Option<ShownRecordings>,
}

#[derive(Default)]
struct HeldUses {
    next: u64,
    entries: Vec<Entry>,
}

struct Entry {
    id: u64,
    holding: Holding,
    since: Instant,
    /// The time of day it started, which its session is kept by.
    started: SystemTime,
}

/// A file the program has open, while anything holds its handle.
struct OpenFile {
    handle: Weak<fs::File>,
    kind: MountKind,
    mode: OpenMode,
}

/// What a program holds, read at one moment.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the live state is read by the inspection protocol, a later step"
    )
)]
pub(crate) struct Holdings {
    pub(crate) held: Vec<HeldNow>,
    pub(crate) open_files: Vec<OpenFileNow>,
}

/// A use held, and since when.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the live state is read by the inspection protocol, a later step"
    )
)]
pub(crate) struct HeldNow {
    pub(crate) holding: Holding,
    pub(crate) since: Instant,
}

/// A file open: where the system says it is now, the kind of mount it was opened in, and what
/// for.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the live state is read by the inspection protocol, a later step"
    )
)]
pub(crate) struct OpenFileNow {
    pub(crate) path: Option<PathBuf>,
    pub(crate) kind: MountKind,
    pub(crate) mode: OpenMode,
}

impl Registry {
    pub(super) fn new(shown: Option<ShownRecordings>) -> Registry {
        Registry {
            held: Mutex::default(),
            open_files: Mutex::default(),
            shown,
        }
    }

    /// Adds `holding`, telling the indicator where it is the first recording of its kind.
    pub(super) fn hold(parts: &Arc<AccessParts>, holding: Holding, instance: u32) -> Held {
        let registry = &parts.holdings;
        let mut held = locked(&registry.held);
        if let Holding::Recording(device) = holding
            && !held.records_with(device)
        {
            registry.show(device, true);
        }
        let id = held.next;
        held.next += 1;
        held.entries.push(Entry {
            id,
            holding,
            since: Instant::now(),
            started: SystemTime::now(),
        });
        Held {
            parts: Arc::clone(parts),
            id,
            instance,
        }
    }

    /// Takes away the entry `id`, telling the indicator where it was the last recording of its
    /// kind, and answers it.
    fn end(&self, id: u64) -> Option<Entry> {
        let mut held = locked(&self.held);
        let at = held.entries.iter().position(|entry| entry.id == id)?;
        let ended = held.entries.remove(at);
        if let Holding::Recording(device) = ended.holding
            && !held.records_with(device)
        {
            self.show(device, false);
        }
        Some(ended)
    }

    /// Tells the indicator, where there is one, whether the program records with `device`.
    fn show(&self, device: RecordingDevice, recording: bool) {
        if let Some(shown) = &self.shown {
            shown
                .indicator
                .recording_changed(&shown.program, device, recording);
        }
    }

    /// Adds `file` to the open files, the closed ones swept as the list fills.
    pub(super) fn opened(&self, file: &Arc<fs::File>, kind: MountKind, mode: OpenMode) {
        let mut open_files = locked(&self.open_files);
        if open_files.len() == open_files.capacity() {
            open_files.retain(|open| open.handle.strong_count() > 0);
        }
        open_files.push(OpenFile {
            handle: Arc::downgrade(file),
            kind,
            mode,
        });
    }

    /// What the program holds now, each open file where the system says it is.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the live state is read by the inspection protocol, a later step"
        )
    )]
    pub(super) fn now(&self) -> Holdings {
        let held = locked(&self.held)
            .entries
            .iter()
            .map(|entry| HeldNow {
                holding: entry.holding.clone(),
                since: entry.since,
            })
            .collect();
        let open_files = locked(&self.open_files)
            .iter()
            .filter_map(|open| {
                let handle = open.handle.upgrade()?;
                Some(OpenFileNow {
                    path: crate::files::path_of(&handle).ok(),
                    kind: open.kind,
                    mode: open.mode,
                })
            })
            .collect();
        Holdings { held, open_files }
    }
}

impl HeldUses {
    fn records_with(&self, device: RecordingDevice) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.holding, Holding::Recording(held) if held == device))
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(ended) = self.parts.holdings.end(self.id) {
            self.parts
                .tell_ended(&ended.holding, (ended.started, ended.since), self.instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ProgramAccess, Told};
    use super::*;
    use crate::permissions::NothingGranted;

    fn access() -> ProgramAccess {
        ProgramAccess::new(Arc::new(NothingGranted))
    }

    #[test]
    fn a_use_is_held_until_its_hold_is_dropped() {
        let access = access();
        let camera = access.hold(Holding::Recording(RecordingDevice::Camera));
        let run = access.hold(Holding::Run {
            machine: "This Mac".to_owned(),
            program: "git".to_owned(),
            in_terminal: false,
        });
        let held = access.holdings().held;
        assert_eq!(held.len(), 2);
        assert!(held.iter().all(|now| now.since <= Instant::now()));
        drop(camera);
        let held = access.holdings().held;
        assert!(matches!(
            held[..],
            [HeldNow {
                holding: Holding::Run { .. },
                ..
            }]
        ));
        drop(run);
        assert!(access.holdings().held.is_empty());
        assert_eq!(access.told().last(), Some(&Told::Ended));
    }

    #[test]
    fn a_file_is_open_while_its_handle_is_held_and_named_where_the_system_says() {
        let access = access();
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("note.txt");
        fs::write(&path, "").unwrap();
        let file = Arc::new(fs::File::open(&path).unwrap());
        let mode = OpenMode {
            read: true,
            write: false,
        };
        access.opened(&file, MountKind::Granted, mode);
        let renamed = scratch.path().join("renamed.txt");
        fs::rename(&path, &renamed).unwrap();
        let open_files = access.holdings().open_files;
        assert_eq!(open_files.len(), 1);
        assert_eq!(
            (open_files[0].kind, open_files[0].mode),
            (MountKind::Granted, mode)
        );
        assert_eq!(
            fs::canonicalize(open_files[0].path.as_ref().unwrap()).unwrap(),
            fs::canonicalize(&renamed).unwrap(),
            "the system names it where it is now"
        );
        drop(file);
        assert!(access.holdings().open_files.is_empty());
    }
}
