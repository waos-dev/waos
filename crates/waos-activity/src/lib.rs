//! What a program did over the days WaOS keeps of it, summed across the days and said in plain
//! words: one [`Section`] per capability for the inspector's Activity, and a line per capability,
//! in short, for a program's settings page, so both say the same of the same days.
//!
//! The invariant: everything said is said here, of an [`Activity`] its users fill from their own
//! types of a day, and nothing here names those types, so the words never depend on which side
//! reads the days.

mod dates;
mod words;

pub use dates::when;

/// What a program did over the days kept, summed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub camera: Captures,
    pub microphone: Captures,
    /// The sites it reached, by domain, merged across the days.
    pub sites: Vec<Named>,
    /// This computer and the local network, by address.
    pub local_addresses: Vec<Named>,
    pub folders: Vec<Folder>,
    /// The opens and changes its folders refused.
    pub folder_refusals: u32,
    /// The clipboard read without the user's paste.
    pub clipboard_reads: Counts,
    pub pastes: u32,
    pub clipboard_writes: Counts,
    /// What it ran, each command on its machine.
    pub commands: Vec<CommandRuns>,
    pub links: Vec<Named>,
    pub notifications: Counts,
    pub questions: Vec<Asked>,
    /// When it last used anything, in seconds since the epoch.
    pub last_use: Option<i64>,
}

/// A camera's or the microphone's sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Captures {
    pub sessions: u32,
    pub seconds: u64,
    /// Whether every session captured from one of WaOS's own devices.
    pub all_simulated: bool,
}

/// How often a use was allowed, and how often refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub allowed: u32,
    pub refused: u32,
}

/// The uses of one thing, by its name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    pub uses: Counts,
}

/// The runs of one command on one machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandRuns {
    pub machine: String,
    pub command: String,
    pub uses: Counts,
}

/// What a program did in one of its folders, where it is on the machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folder {
    pub path: String,
    pub opened_to_read: u32,
    pub opened_to_write: u32,
    /// Files created, removed or renamed in it.
    pub changes: u32,
    /// The files changed in it, as far as they were listed.
    pub changed: Vec<String>,
}

/// A question put to the user, by its row's key, when, and the answer, by its key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub question: String,
    pub at: i64,
    pub answer: Option<String>,
}

/// What a program can use that needs its user's permission, in the order its user worries about
/// them: what records, the network, files, the clipboard, machines, links, notifications and
/// questions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AccessKind {
    Camera,
    Microphone,
    Network,
    Files,
    Clipboard,
    Machines,
    Links,
    Notifications,
    Questions,
}

impl AccessKind {
    pub const ALL: [AccessKind; 9] = [
        AccessKind::Camera,
        AccessKind::Microphone,
        AccessKind::Network,
        AccessKind::Files,
        AccessKind::Clipboard,
        AccessKind::Machines,
        AccessKind::Links,
        AccessKind::Notifications,
        AccessKind::Questions,
    ];

    /// Its name, as a list of programs marks it: `Camera`, `Machines`…
    pub fn name(self) -> &'static str {
        match self {
            AccessKind::Camera => "Camera",
            AccessKind::Microphone => "Microphone",
            AccessKind::Network => "Network",
            AccessKind::Files => "Files",
            AccessKind::Clipboard => "Clipboard",
            AccessKind::Machines => "Machines",
            AccessKind::Links => "Links",
            AccessKind::Notifications => "Notifications",
            AccessKind::Questions => "Questions",
        }
    }

    /// What its section is titled, before what it did: `Ran` for the machines, `Asked` for the
    /// questions, its name for the rest.
    pub fn heading(self) -> &'static str {
        match self {
            AccessKind::Machines => "Ran",
            AccessKind::Questions => "Asked",
            kind => kind.name(),
        }
    }
}

/// One capability's part of what a program did: which, and what it did, a line each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub kind: AccessKind,
    pub lines: Vec<String>,
}

impl Activity {
    /// A camera's or the microphone's session of `seconds` added to `captures`, its device WaOS's
    /// own where `simulated`.
    pub fn add_session(captures: &mut Captures, seconds: u32, simulated: bool) {
        captures.all_simulated = (captures.sessions == 0 || captures.all_simulated) && simulated;
        captures.sessions += 1;
        captures.seconds += u64::from(seconds);
    }

    /// `uses` of `name` among `list`, added to what it holds of that name.
    pub fn add_named(list: &mut Vec<Named>, name: &str, uses: Counts) {
        match list.iter_mut().find(|named| named.name == name) {
            Some(named) => {
                named.uses.allowed += uses.allowed;
                named.uses.refused += uses.refused;
            }
            None => list.push(Named {
                name: name.to_owned(),
                uses,
            }),
        }
    }

    /// `uses` of `command` on `machine`, added to what it holds of them.
    pub fn add_command(&mut self, machine: &str, command: &str, uses: Counts) {
        let kept = self
            .commands
            .iter_mut()
            .find(|runs| runs.machine == machine && runs.command == command);
        match kept {
            Some(runs) => {
                runs.uses.allowed += uses.allowed;
                runs.uses.refused += uses.refused;
            }
            None => self.commands.push(CommandRuns {
                machine: machine.to_owned(),
                command: command.to_owned(),
                uses,
            }),
        }
    }

    /// What was done in the folder at `path`, added to what it holds of that folder.
    pub fn add_folder(&mut self, folder: Folder) {
        match self
            .folders
            .iter_mut()
            .find(|kept| kept.path == folder.path)
        {
            Some(kept) => {
                kept.opened_to_read += folder.opened_to_read;
                kept.opened_to_write += folder.opened_to_write;
                kept.changes += folder.changes;
                for path in folder.changed {
                    if !kept.changed.contains(&path) {
                        kept.changed.push(path);
                    }
                }
            }
            None => self.folders.push(folder),
        }
    }

    /// A use at `at`, in seconds since the epoch, which may be its last.
    pub fn used_at(&mut self, at: i64) {
        self.last_use = Some(self.last_use.map_or(at, |last| last.max(at)));
    }

    /// One section per capability it used, in plain words, in the order its user worries about
    /// them: what records, the network, files, the clipboard, machines, links, notifications and
    /// questions.
    pub fn sections(&self) -> Vec<Section> {
        [
            words::captures(AccessKind::Camera, &self.camera),
            words::captures(AccessKind::Microphone, &self.microphone),
            words::network(self),
            words::files(self),
            words::clipboard(self),
            words::commands(self),
            words::links(self),
            words::notifications(self),
            words::questions(self),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// A line per capability it used, in short: what a program's settings page shows under its
    /// switches.
    pub fn short(&self) -> Vec<String> {
        self.sections()
            .into_iter()
            .filter_map(|section| {
                let first = section.lines.into_iter().next()?;
                Some(format!("{}: {first}", section.kind.heading()))
            })
            .collect()
    }

    /// What it used, as a list of programs marks each: what records, the network, files, the
    /// clipboard and machines.
    pub fn marks(&self) -> Vec<AccessKind> {
        let used = [
            (AccessKind::Camera, self.camera.sessions > 0),
            (AccessKind::Microphone, self.microphone.sessions > 0),
            (
                AccessKind::Network,
                !self.sites.is_empty() || !self.local_addresses.is_empty(),
            ),
            (
                AccessKind::Files,
                !self.folders.is_empty() || self.folder_refusals > 0,
            ),
            (
                AccessKind::Clipboard,
                self.clipboard_reads != Counts::default()
                    || self.pastes > 0
                    || self.clipboard_writes != Counts::default(),
            ),
            (AccessKind::Machines, !self.commands.is_empty()),
        ];
        used.into_iter()
            .filter_map(|(mark, used)| used.then_some(mark))
            .collect()
    }

    /// Whether it used nothing at all.
    pub fn is_empty(&self) -> bool {
        self.sections().is_empty()
    }
}
