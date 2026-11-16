//! What a program did with what it was allowed, one day at a time ([`AuditDay`]), and where those
//! days are kept ([`AuditLog`]): the audit summary of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`, counted by the engine and
//! kept by the store, on the device only, seven days.
//!
//! A day is small and bounded whatever the program does: counts, refusals, first and last use, and
//! short lists, each capped ([`LIST_CAPS`]), what falls beyond a cap one count in [`Overflow`].
//!
//! The invariant: a day holds no content of a use, only that it happened: no file's contents, no
//! clipboard's, no command's arguments.

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// How many days of a program are kept.
pub const DAYS_KEPT: usize = 7;

/// How long each of a day's lists may grow, beyond which a use is one count in [`Overflow`].
pub const LIST_CAPS: ListCaps = ListCaps {
    domains: 256,
    local_addresses: 256,
    folders: 64,
    changed: 100,
    commands: 100,
    links: 100,
    device_sessions: 100,
    questions: 100,
};

/// Where a program's audit summary is kept: the store, for the WaOS app.
pub trait AuditLog: Send + Sync {
    /// Keeps `day`, `program`'s uses over one day so far, in place of what was kept for that day.
    fn keep(&self, program: &str, day: &AuditDay);

    /// The days kept of `program`, newest first, [`DAYS_KEPT`] at most.
    fn days(&self, program: &str) -> Vec<AuditDay>;
}

/// One program's guarded uses over one day, from midnight UTC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditDay {
    pub start: SystemTime,
    /// Per domain: the program's own lookups, its connections by the names their address was
    /// looked up by, its requests, and the links its players open.
    pub domains: Vec<NamedUses<String>>,
    /// This computer and the local network, per address.
    pub local_addresses: Vec<NamedUses<IpAddr>>,
    /// The UDP datagrams sent and received, refused ones among them.
    pub udp_datagrams: u64,
    pub folders: Vec<FolderUses>,
    /// The opens and changes the mounts refused: outside every folder, or a change a read-only
    /// one does not allow.
    pub folder_refusals: u32,
    /// The clipboard read without the user's paste.
    pub clipboard_reads: Uses,
    /// The clipboard read as the user pasted it.
    pub pastes: u32,
    pub clipboard_writes: Uses,
    pub camera: Vec<DeviceSession>,
    pub microphone: Vec<DeviceSession>,
    pub commands: Vec<NamedUses<MachineCommand>>,
    /// Per scheme and host, `https://example.com`, or scheme alone, `mailto`.
    pub links: Vec<NamedUses<String>>,
    pub notifications: Uses,
    pub questions: Vec<AskedQuestion>,
    pub beyond_caps: Overflow,
}

/// How often a use happened, how often it was refused, and when it first and last happened; no
/// time while it has not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uses {
    pub allowed: u32,
    pub refused: u32,
    pub first: Option<SystemTime>,
    pub last: Option<SystemTime>,
}

/// What was used, and how.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedUses<T> {
    pub name: T,
    pub uses: Uses,
}

/// What a program is to one of its mounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MountKind {
    OwnData,
    OwnCache,
    OwnTemporary,
    Granted,
    Picked,
    Pasted,
    Machine,
}

impl MountKind {
    /// Whether the mount is the user's: a folder granted, a pick or a paste, whose changes a day
    /// lists by path.
    pub fn is_the_users(self) -> bool {
        matches!(
            self,
            MountKind::Granted | MountKind::Picked | MountKind::Pasted
        )
    }
}

/// What a program did in one mount.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderUses {
    /// Where the mount is on the machine.
    pub path: PathBuf,
    pub kind: MountKind,
    pub opened_to_read: u32,
    pub opened_to_write: u32,
    pub created: u32,
    pub removed: u32,
    pub renamed: u32,
    /// The files changed in it, as the machine names them: in the user's mounts only, the day's
    /// [`ListCaps::changed`] in all.
    pub changed: Vec<PathBuf>,
}

/// A camera or microphone session: when it started, how long it ran, and whether it was WaOS's
/// own device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSession {
    pub start: SystemTime,
    pub seconds: u32,
    pub simulated: bool,
}

/// A command a program ran: on which machine, and the program it ran, never its arguments.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MachineCommand {
    pub machine: String,
    pub command: String,
}

/// A question put to the user: by its row's key (`camera`, `link-scheme.zoommtg`), when, and its
/// answer, by its key; none while nobody answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskedQuestion {
    pub question: String,
    pub at: SystemTime,
    pub answer: Option<String>,
}

/// What fell beyond each list's cap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overflow {
    pub domains: u32,
    pub local_addresses: u32,
    pub folders: u32,
    pub changed: u32,
    pub commands: u32,
    pub links: u32,
    pub device_sessions: u32,
    pub questions: u32,
}

/// How long each list of a day may grow.
#[derive(Clone, Copy, Debug)]
pub struct ListCaps {
    pub domains: usize,
    pub local_addresses: usize,
    pub folders: usize,
    pub changed: usize,
    pub commands: usize,
    pub links: usize,
    pub device_sessions: usize,
    pub questions: usize,
}

impl AuditDay {
    /// A day with nothing in it yet, from `start`.
    pub fn starting(start: SystemTime) -> AuditDay {
        AuditDay {
            start,
            domains: Vec::new(),
            local_addresses: Vec::new(),
            udp_datagrams: 0,
            folders: Vec::new(),
            folder_refusals: 0,
            clipboard_reads: Uses::default(),
            pastes: 0,
            clipboard_writes: Uses::default(),
            camera: Vec::new(),
            microphone: Vec::new(),
            commands: Vec::new(),
            links: Vec::new(),
            notifications: Uses::default(),
            questions: Vec::new(),
            beyond_caps: Overflow::default(),
        }
    }
}

impl Uses {
    /// Counts one use at `at`, allowed or refused.
    pub fn count(&mut self, allowed: bool, at: SystemTime) {
        if allowed {
            self.allowed += 1;
        } else {
            self.refused += 1;
        }
        self.first.get_or_insert(at);
        self.last = Some(at);
    }
}
