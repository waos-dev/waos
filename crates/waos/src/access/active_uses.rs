//! What a program uses now: each use that lasts, from the moment what owns the real thing starts it
//! until that thing really ends (a capture session, the microphone recording, a command until it
//! exits, an HTTP request until its response's body ends, a folder watched, a TCP connection,
//! listener or UDP socket until the system's socket closes), and each file it has open, until its
//! last handle goes. One registry holds them all, each entry added as its [`ActiveUse`] is made and
//! taken away as it is dropped, wherever that is, the program ending among the ways. The shell's
//! indicator of who records reads the same registry: it hears a program records with a kind of
//! device as its first one starts, and that it stopped as its last ends. Whoever listens is given
//! what is active as it subscribes, then each start and end; one that falls behind is sent the
//! whole of it again in place of what it missed, so it never shows a use that ended, and the
//! program never waits for it. A listener is also told what each TCP connection and UDP socket
//! moved, at most once a second for each and only as it moved more: its own stream reads the counts
//! on its own tick, so a program nobody listens to pays nothing for it.
//!
//! An entry is a lock, a map insertion and two clocks read, beside work far slower; an open
//! file's path is asked of the system only for a listener, and outside the lock: before it for a
//! file that opens, after it for a listener's snapshot or resync, which may hold thousands; and
//! the bytes a socket moved only when someone reads the entry (measured: with nobody listening an
//! entry costs nothing beside an open, and a listener makes each open about 3 µs slower).
//!
//! The invariant: an entry goes exactly when its [`ActiveUse`] is dropped, and every change to the
//! entries is sent to the listeners under the same lock that makes it, so a listener's snapshot
//! and the changes after it say the same as the entries; a listener is told a use's traffic only
//! between its start and its end, as the listener heard them.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};

use futures::Stream;
use futures::channel::mpsc;

use super::AccessParts;
use crate::files::{MountKind, OpenMode};
use crate::network::{Counts, Datagrams};
use crate::running::AppInstance;
use crate::shell::{RecordingDevice, ShellRecordingIndicator};
use crate::sync::locked;

/// How many changes wait for a listener before it is behind, and is sent the whole again.
const LISTENER_CAPACITY: usize = 1024;

/// How often a listener reads what the connections and sockets moved.
const TRAFFIC_EVERY: Duration = Duration::from_secs(1);

/// A use the program makes now.
#[derive(Clone, Debug)]
pub(crate) enum ActiveUseKind {
    /// A capture session or the microphone recording, as the indicator shows it.
    Recording(RecordingDevice),
    /// A command running on a machine.
    Command {
        machine: String,
        command: String,
        in_terminal: bool,
    },
    /// An HTTP request in flight, until its response's body ends.
    HttpRequest {
        method: http::Method,
        uri: http::Uri,
    },
    /// A folder watched, where it was on the machine when the watch began.
    FolderWatch { folder: PathBuf, recursive: bool },
    /// A file or folder open, while its handle is held: the handle, the kind of mount it was
    /// opened in, and what for.
    OpenFile {
        file: Weak<fs::File>,
        mount: MountKind,
        mode: OpenMode,
        /// Where the system says it is, read from the handle as the use is told to a listener,
        /// while the handle is surely there; none in the registry, and none where the system
        /// cannot say.
        at: Option<PathBuf>,
    },
    /// A TCP connection, until its socket closes.
    TcpConnection {
        origin: TcpOrigin,
        /// This end, where the system said it as the connection was made.
        local: Option<SocketAddr>,
        /// The other end.
        remote: SocketAddr,
        /// The names `remote` was looked up by as the connection was made.
        names: Vec<String>,
        /// What it moved, read as the entry is.
        counts: Arc<Counts>,
    },
    /// A TCP socket listening at `address`, until it closes.
    TcpListener { address: SocketAddr },
    /// A UDP socket, until it closes: its addresses and what it moved, read as the entry is.
    UdpSocket { socket: Weak<Datagrams> },
}

/// How a TCP connection came to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TcpOrigin {
    /// The program connected.
    Connected,
    /// The program's listener accepted it.
    Accepted,
    /// The engine connected, to send the program's HTTP requests.
    HttpRequest,
}

/// Who shows a program's recordings: the shell's indicator, which names the program.
pub(crate) struct ShownRecordings {
    pub(crate) indicator: Arc<dyn ShellRecordingIndicator>,
    pub(crate) program: AppInstance,
}

/// A use active from its start until this is dropped.
#[must_use = "the use ends as it is dropped"]
pub(crate) struct ActiveUse {
    parts: Arc<AccessParts>,
    id: u64,
    /// The program's instance that makes it, which its end is told in.
    instance: u32,
}

/// A use active now: its id among the program's, what it is, and since when.
#[derive(Clone, Debug)]
pub(crate) struct ActiveUseNow {
    pub(crate) id: u64,
    pub(crate) kind: ActiveUseKind,
    pub(crate) since: Instant,
}

/// A change to what a program uses now, as a listener hears it.
#[derive(Clone, Debug)]
pub(crate) enum ActiveUseChange {
    Started(ActiveUseNow),
    /// The use of this id ended.
    Ended(u64),
    /// Everything active now, in place of the changes the listener fell behind on.
    Resync(Vec<ActiveUseNow>),
    /// What a connection or a socket moved so far.
    Updated(Traffic),
}

/// What a TCP connection or a UDP socket moved so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Traffic {
    pub(crate) id: u64,
    pub(crate) sent: u64,
    pub(crate) received: u64,
}

/// One program's registry of what it uses now.
pub(super) struct ActiveUses {
    entries: Mutex<Entries>,
    /// How many listen, read without the lock, so a use nobody listens to asks the system
    /// nothing.
    listening: AtomicUsize,
    shown: Option<ShownRecordings>,
}

#[derive(Default)]
struct Entries {
    next: u64,
    active: BTreeMap<u64, Entry>,
    listeners: Vec<Listener>,
}

struct Entry {
    kind: ActiveUseKind,
    since: Instant,
    /// The time of day it started, which a recording's session is kept by.
    started: SystemTime,
}

struct Listener {
    changes: mpsc::Sender<ActiveUseChange>,
    /// Whether it lost a change, and is to be sent the whole again.
    behind: Arc<std::sync::atomic::AtomicBool>,
}

/// One listener's changes to what the program uses now, from what was active as it subscribed,
/// until the program and every use it made have ended; dropped, it no longer listens.
pub(crate) struct ActiveUseChanges {
    changes: mpsc::Receiver<ActiveUseChange>,
    behind: Arc<std::sync::atomic::AtomicBool>,
    /// The registry, held without keeping it: it goes with the program's access, and the
    /// changes end then.
    parts: Weak<AccessParts>,
    traffic: TrafficTold,
}

/// What a listener was told each connection and socket moved, and when it reads them next.
#[derive(Default)]
struct TrafficTold {
    /// Each connection's and socket's bytes sent and received, as last told, by its id.
    told: HashMap<u64, (u64, u64)>,
    /// Made as the listener is first read, on the runtime that reads it.
    tick: Option<tokio::time::Interval>,
    /// What the last reading found moved, not given to the listener yet.
    waiting: VecDeque<Traffic>,
}

/// Where a connection's or a socket's bytes are read, copied under the registry's lock and read
/// after it.
enum Moving {
    Connection(Arc<Counts>),
    Socket(Weak<Datagrams>),
}

impl ActiveUses {
    pub(super) fn new(shown: Option<ShownRecordings>) -> ActiveUses {
        ActiveUses {
            entries: Mutex::default(),
            listening: AtomicUsize::new(0),
            shown,
        }
    }

    /// Adds `kind`, telling the indicator where it is the first recording of its kind, and each
    /// listener.
    pub(super) fn start(parts: &Arc<AccessParts>, kind: ActiveUseKind, instance: u32) -> ActiveUse {
        let registry = &parts.active_uses;
        // Named before the lock is taken: an open file is asked of the system while it is surely
        // open, since a listener reads what it is told later.
        let listened = registry.listening.load(Ordering::Relaxed) > 0;
        let told = listened.then(|| named(kind.clone()));
        let mut entries = locked(&registry.entries);
        if let ActiveUseKind::Recording(device) = kind
            && !entries.records_with(device)
        {
            registry.show(device, true);
        }
        let id = entries.next;
        entries.next += 1;
        let entry = Entry {
            kind,
            since: Instant::now(),
            started: SystemTime::now(),
        };
        if !entries.listeners.is_empty() {
            let started = ActiveUseChange::Started(ActiveUseNow {
                id,
                kind: told.unwrap_or_else(|| entry.kind.clone()),
                since: entry.since,
            });
            entries.send(&started);
            registry.count_listeners(&entries);
        }
        entries.active.insert(id, entry);
        ActiveUse {
            parts: Arc::clone(parts),
            id,
            instance,
        }
    }

    /// Takes away the entry `id`, telling the indicator where it was the last recording of its
    /// kind, and each listener, and answers it.
    fn end(&self, id: u64) -> Option<Entry> {
        let mut entries = locked(&self.entries);
        let ended = entries.active.remove(&id)?;
        if let ActiveUseKind::Recording(device) = ended.kind
            && !entries.records_with(device)
        {
            self.show(device, false);
        }
        if !entries.listeners.is_empty() {
            entries.send(&ActiveUseChange::Ended(id));
            self.count_listeners(&entries);
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

    /// What the program uses now, oldest first.
    #[cfg(test)]
    pub(super) fn now(&self) -> Vec<ActiveUseNow> {
        locked(&self.entries).now()
    }

    /// What the program uses now, and each change from then on.
    pub(super) fn subscribe(parts: &Arc<AccessParts>) -> (Vec<ActiveUseNow>, ActiveUseChanges) {
        let (changes, received) = mpsc::channel(LISTENER_CAPACITY);
        let behind = Arc::default();
        let now = {
            let mut entries = locked(&parts.active_uses.entries);
            entries.listeners.push(Listener {
                changes,
                behind: Arc::clone(&behind),
            });
            parts.active_uses.count_listeners(&entries);
            entries.now()
        };
        let mut changes = ActiveUseChanges {
            changes: received,
            behind,
            parts: Arc::downgrade(parts),
            traffic: TrafficTold::default(),
        };
        // Each open file named after the lock is let go: there may be thousands.
        let now: Vec<ActiveUseNow> = now.into_iter().map(named_now).collect();
        changes.traffic.heard_all(&now);
        (now, changes)
    }

    /// Notes how many listen now.
    fn count_listeners(&self, entries: &Entries) {
        self.listening
            .store(entries.listeners.len(), Ordering::Relaxed);
    }
}

/// `now`, an open file named where the system says it is.
fn named_now(now: ActiveUseNow) -> ActiveUseNow {
    ActiveUseNow {
        kind: named(now.kind),
        ..now
    }
}

/// `kind`, an open file named where the system says it is now; asks the system, so it is called
/// outside the registry's lock.
fn named(mut kind: ActiveUseKind) -> ActiveUseKind {
    if let ActiveUseKind::OpenFile { file, at, .. } = &mut kind {
        *at = file
            .upgrade()
            .and_then(|file| crate::files::path_of(&file).ok());
    }
    kind
}

impl Entries {
    fn records_with(&self, device: RecordingDevice) -> bool {
        self.active
            .values()
            .any(|entry| matches!(entry.kind, ActiveUseKind::Recording(held) if held == device))
    }

    fn now(&self) -> Vec<ActiveUseNow> {
        self.active
            .iter()
            .map(|(id, entry)| entry.now(*id))
            .collect()
    }

    /// Sends `change` to each listener that keeps up; one whose changes are full is behind from
    /// now on, sent nothing until it is sent the whole again, and one gone is forgotten.
    fn send(&mut self, change: &ActiveUseChange) {
        self.listeners.retain_mut(|listener| {
            if listener.behind.load(std::sync::atomic::Ordering::Relaxed) {
                return !listener.changes.is_closed();
            }
            match listener.changes.try_send(change.clone()) {
                Ok(()) => true,
                Err(full) if full.is_full() => {
                    listener
                        .behind
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    true
                }
                Err(_gone) => false,
            }
        });
    }
}

impl Entry {
    /// The entry as it is held; an open file is named by [`named`], outside the lock.
    fn now(&self, id: u64) -> ActiveUseNow {
        ActiveUseNow {
            id,
            kind: self.kind.clone(),
            since: self.since,
        }
    }
}

impl Stream for ActiveUseChanges {
    type Item = ActiveUseChange;

    /// The next change; once every change sent before it fell behind is read, the whole as it is
    /// now, taken under the lock that sends, so nothing between is lost; and between them, once a
    /// second, what each connection and socket moved since the listener was last told.
    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<ActiveUseChange>> {
        let this = &mut *self;
        if let Some(traffic) = this.traffic.waiting.pop_front() {
            return Poll::Ready(Some(ActiveUseChange::Updated(traffic)));
        }
        let change = match Pin::new(&mut this.changes).poll_next(context) {
            Poll::Ready(change) => Some(change),
            Poll::Pending if this.behind.load(std::sync::atomic::Ordering::Relaxed) => {
                let Some(parts) = this.parts.upgrade() else {
                    return Poll::Ready(None);
                };
                let now = {
                    let entries = locked(&parts.active_uses.entries);
                    this.behind
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    entries.now()
                };
                // Named after the lock is let go: a resync may hold thousands of open files.
                let now = now.into_iter().map(named_now).collect();
                Some(Some(ActiveUseChange::Resync(now)))
            }
            Poll::Pending => None,
        };
        if let Some(change) = change {
            if let Some(change) = &change {
                this.traffic.heard(change);
            }
            return Poll::Ready(change);
        }
        match this.traffic.poll_moved(context, &this.parts) {
            Some(traffic) => Poll::Ready(Some(ActiveUseChange::Updated(traffic))),
            None => Poll::Pending,
        }
    }
}

impl TrafficTold {
    /// The listener heard `change`: a connection or a socket that started is told with its
    /// bytes, one that ended is no longer, and a resync tells them all again.
    fn heard(&mut self, change: &ActiveUseChange) {
        match change {
            ActiveUseChange::Started(started) => self.heard_one(started),
            ActiveUseChange::Ended(id) => {
                self.told.remove(id);
                self.waiting.retain(|traffic| traffic.id != *id);
            }
            ActiveUseChange::Resync(now) => self.heard_all(now),
            ActiveUseChange::Updated(_) => {}
        }
    }

    /// The listener heard `now` as all that is active.
    fn heard_all(&mut self, now: &[ActiveUseNow]) {
        self.told.clear();
        self.waiting.clear();
        for active in now {
            self.heard_one(active);
        }
    }

    fn heard_one(&mut self, active: &ActiveUseNow) {
        if let Some(moved) = Moving::of(&active.kind).and_then(|moving| moving.moved()) {
            self.told.insert(active.id, moved);
        }
    }

    /// What moved since the listener was last told, read once its tick comes; the rest of what
    /// moved waits for its next read. A listener that knows no connection or socket has no tick,
    /// and one that does reads on the tokio runtime that polls it, as every listener's is.
    fn poll_moved(
        &mut self,
        context: &mut Context<'_>,
        parts: &Weak<AccessParts>,
    ) -> Option<Traffic> {
        if self.told.is_empty() {
            return None;
        }
        let mut tick = self.tick.take().unwrap_or_else(|| {
            let start = tokio::time::Instant::now() + TRAFFIC_EVERY;
            let mut tick = tokio::time::interval_at(start, TRAFFIC_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick
        });
        let mut moved = None;
        while moved.is_none() && tick.poll_tick(context).is_ready() {
            let Some(parts) = parts.upgrade() else {
                break;
            };
            self.read_moved(&parts);
            moved = self.waiting.pop_front();
        }
        self.tick = Some(tick);
        moved
    }

    /// Notes, to tell, each connection and socket the listener knows that moved more.
    fn read_moved(&mut self, parts: &AccessParts) {
        // Copied under the lock and read after it: there may be thousands.
        let moving: Vec<(u64, Moving)> = {
            let entries = locked(&parts.active_uses.entries);
            (self.told.keys())
                .filter_map(|id| {
                    let entry = entries.active.get(id)?;
                    Some((*id, Moving::of(&entry.kind)?))
                })
                .collect()
        };
        for (id, moving) in moving {
            let Some((sent, received)) = moving.moved() else {
                continue;
            };
            let told = self.told.insert(id, (sent, received));
            if told != Some((sent, received)) {
                self.waiting.push_back(Traffic { id, sent, received });
            }
        }
    }
}

impl Moving {
    /// Where `kind` counts what it moved: a TCP connection or a UDP socket.
    fn of(kind: &ActiveUseKind) -> Option<Moving> {
        match kind {
            ActiveUseKind::TcpConnection { counts, .. } => {
                Some(Moving::Connection(Arc::clone(counts)))
            }
            ActiveUseKind::UdpSocket { socket } => Some(Moving::Socket(Weak::clone(socket))),
            _ => None,
        }
    }

    /// The bytes sent and received so far; none for a socket gone.
    fn moved(&self) -> Option<(u64, u64)> {
        match self {
            Moving::Connection(counts) => Some((counts.sent(), counts.received())),
            Moving::Socket(socket) => {
                let socket = socket.upgrade()?;
                Some((socket.counts().sent(), socket.counts().received()))
            }
        }
    }
}

impl Drop for ActiveUse {
    fn drop(&mut self) {
        if let Some(ended) = self.parts.active_uses.end(self.id) {
            self.parts
                .tell_ended(&ended.kind, (ended.started, ended.since), self.instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::{FutureExt, StreamExt};

    use super::super::{ProgramAccess, Told};
    use super::*;
    use crate::permissions::NothingGranted;

    fn access() -> ProgramAccess {
        ProgramAccess::new(Arc::new(NothingGranted))
    }

    fn command() -> ActiveUseKind {
        ActiveUseKind::Command {
            machine: "This Mac".to_owned(),
            command: "git".to_owned(),
            in_terminal: false,
        }
    }

    /// The changes waiting for `changes` now, in order.
    fn waiting(changes: &mut ActiveUseChanges) -> Vec<ActiveUseChange> {
        std::iter::from_fn(|| changes.next().now_or_never().flatten()).collect()
    }

    #[test]
    fn a_use_is_active_until_its_guard_is_dropped() {
        let access = access();
        let camera = access.hold(ActiveUseKind::Recording(RecordingDevice::Camera));
        let command = access.hold(command());
        assert_eq!(access.uses_but_open_files().len(), 2);
        drop(camera);
        assert!(matches!(
            access.uses_but_open_files()[..],
            [ActiveUseKind::Command { .. }]
        ));
        drop(command);
        assert!(access.uses_but_open_files().is_empty());
        assert_eq!(access.told().last(), Some(&Told::Ended));
    }

    #[test]
    fn a_file_is_open_until_its_use_goes_and_named_where_the_system_says() {
        let access = access();
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("note.txt");
        fs::write(&path, "").unwrap();
        let file = Arc::new(fs::File::open(&path).unwrap());
        let mode = OpenMode {
            read: true,
            write: false,
        };
        let open = access.opened(&file, MountKind::Granted, mode);
        let renamed = scratch.path().join("renamed.txt");
        fs::rename(&path, &renamed).unwrap();
        let open_files = access.open_files();
        assert_eq!(open_files.len(), 1);
        assert_eq!(
            (open_files[0].1, open_files[0].2),
            (MountKind::Granted, mode)
        );
        assert_eq!(
            fs::canonicalize(open_files[0].0.as_ref().unwrap()).unwrap(),
            fs::canonicalize(&renamed).unwrap(),
            "the system names it where it is now"
        );
        drop(open);
        assert!(access.open_files().is_empty());
        assert!(access.uses_but_open_files().is_empty());
    }

    #[test]
    fn a_listener_hears_what_is_active_then_each_start_and_end() {
        let access = access();
        let before = access.hold(command());
        let (now, mut changes) = access.subscribe_active_uses();
        assert!(matches!(now[..], [ActiveUseNow { id: 0, .. }]));
        let camera = access.hold(ActiveUseKind::Recording(RecordingDevice::Camera));
        drop(before);
        drop(camera);
        let heard = waiting(&mut changes);
        assert!(
            matches!(
                heard[..],
                [
                    ActiveUseChange::Started(ActiveUseNow { id: 1, .. }),
                    ActiveUseChange::Ended(0),
                    ActiveUseChange::Ended(1),
                ]
            ),
            "{heard:?}"
        );
        drop(access);
        assert!(
            matches!(changes.next().now_or_never(), Some(None)),
            "ends with the access"
        );
    }

    #[test]
    fn a_listener_hears_where_a_file_was_opened_though_it_is_closed_before_it_reads() {
        let access = access();
        let (_, mut changes) = access.subscribe_active_uses();
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("feeds.json.saving");
        let file = Arc::new(fs::File::create(&path).unwrap());
        let mode = OpenMode {
            read: false,
            write: true,
        };
        let open = access.opened(&file, MountKind::OwnData, mode);
        drop(open);
        drop(file);
        let heard = waiting(&mut changes);
        let [
            ActiveUseChange::Started(ActiveUseNow {
                kind: ActiveUseKind::OpenFile { at: Some(at), .. },
                ..
            }),
            ActiveUseChange::Ended(_),
        ] = &heard[..]
        else {
            panic!("its start, named, then its end: {heard:?}");
        };
        assert_eq!(
            fs::canonicalize(at).unwrap(),
            fs::canonicalize(&path).unwrap()
        );
    }

    #[test]
    fn a_listener_is_told_what_a_connection_moved_only_as_it_moves_more() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let access = access();
            let counts = Arc::new(Counts::default());
            let connection = access.hold(ActiveUseKind::TcpConnection {
                origin: TcpOrigin::Connected,
                local: None,
                remote: "192.0.2.1:443".parse().unwrap(),
                names: Vec::new(),
                counts: Arc::clone(&counts),
            });
            let (_, mut changes) = access.subscribe_active_uses();
            counts.add_sent(10);
            let moved = changes.next().await;
            assert!(
                matches!(
                    moved,
                    Some(ActiveUseChange::Updated(Traffic {
                        id: 0,
                        sent: 10,
                        received: 0
                    }))
                ),
                "{moved:?}"
            );
            let still = tokio::time::timeout(TRAFFIC_EVERY * 3 / 2, changes.next()).await;
            assert!(
                still.is_err(),
                "nothing told while it moves nothing: {still:?}"
            );
            counts.add_received(5);
            let moved = changes.next().await;
            assert!(
                matches!(
                    moved,
                    Some(ActiveUseChange::Updated(Traffic {
                        id: 0,
                        sent: 10,
                        received: 5
                    }))
                ),
                "{moved:?}"
            );
            drop(connection);
            counts.add_sent(1);
            assert!(matches!(
                changes.next().await,
                Some(ActiveUseChange::Ended(0))
            ));
            let after = tokio::time::timeout(TRAFFIC_EVERY * 3 / 2, changes.next()).await;
            assert!(
                after.is_err(),
                "nothing told of a use that ended: {after:?}"
            );
        });
    }

    #[test]
    fn a_listener_that_falls_behind_is_sent_the_whole_in_place_of_what_it_missed() {
        let access = access();
        let (_, mut changes) = access.subscribe_active_uses();
        let mut uses: Vec<ActiveUse> = (0..LISTENER_CAPACITY + 10)
            .map(|_| access.hold(command()))
            .collect();
        uses.truncate(3);
        let heard = waiting(&mut changes);
        let Some((ActiveUseChange::Resync(active), fitted)) = heard.split_last() else {
            panic!("the last is the whole: {:?}", heard.last());
        };
        assert!(fitted.len() >= LISTENER_CAPACITY, "what fitted came first");
        assert!(
            fitted.len() < LISTENER_CAPACITY + 10,
            "and nothing it missed"
        );
        assert!(
            fitted
                .iter()
                .all(|change| matches!(change, ActiveUseChange::Started(_)))
        );
        let ids: Vec<u64> = active.iter().map(|now| now.id).collect();
        assert_eq!(ids, [0, 1, 2], "never a use that ended");
        drop(uses.pop());
        assert!(
            matches!(waiting(&mut changes)[..], [ActiveUseChange::Ended(2)]),
            "changes again from then on"
        );
    }
}
