//! Every program's day of what it did with what it was allowed, counted in memory as each use is
//! told ([`ProgramDay`]), and the one writer per engine that keeps those days through the
//! engine's [`AuditLog`] ([`Days`]): each day that changed, about once a minute; a day that
//! ended, as the next begins; a program's day at its end; and every day, waited for, as WaOS
//! quits or the engine goes, so a clean quit loses nothing. It writes on a thread of its own,
//! so a guarded use only ever counts in memory, and no program waits on the store's lock, a file
//! write or a sync. The runs of one program count into one day, begun from what was kept of
//! today, so the day counts them all, and an inspector reads today's day from memory.
//!
//! The invariant: only the writer writes a day, and it writes each as it is now, whole, so a day
//! kept is never older than one kept before it; a crash loses at most the last minute.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use waos_capabilities::{AuditDay, AuditLog, ProgramGrants};

use crate::sync::locked;

/// How often each program's day is kept while it changes.
const KEEP_EVERY: Duration = Duration::from_secs(60);

/// A day, in seconds.
const DAY: u64 = 24 * 60 * 60;

/// The days of every program the engine runs, and their writer. Every clone reaches the same
/// days; the writer keeps them all a last time once the last clone is gone.
#[derive(Clone)]
pub(crate) struct Days {
    shared: Arc<Shared>,
    writer: mpsc::Sender<Ask>,
}

/// What the writer is asked besides its minute.
enum Ask {
    /// Keep the day of the program so named: it ended.
    Keep(String),
    /// Keep every day that changed, and say when it is done.
    KeepAll(mpsc::Sender<()>),
}

struct Shared {
    log: Arc<dyn AuditLog>,
    programs: Mutex<HashMap<String, Arc<ProgramDay>>>,
}

/// One program's day, counted in memory.
pub(crate) struct ProgramDay {
    counted: Mutex<Counted>,
    /// The grants of its latest run, which say whether its user keeps a record of it.
    grants: Mutex<Option<Arc<dyn ProgramGrants>>>,
    /// The UDP datagrams counted since the day was last folded, apart from the lock.
    udp_datagrams: AtomicU64,
}

struct Counted {
    day: AuditDay,
    /// Whether the day changed since it was last kept.
    changed: bool,
    /// The days that ended, to be kept.
    ended: Vec<AuditDay>,
}

impl Days {
    /// The days kept through `log`, with the thread that writes them.
    pub(crate) fn new(log: Arc<dyn AuditLog>) -> std::io::Result<Days> {
        let shared = Arc::new(Shared {
            log,
            programs: Mutex::default(),
        });
        let (writer, asked) = mpsc::channel();
        let writing = Arc::clone(&shared);
        thread::Builder::new()
            .name("waos audit days".into())
            .spawn(move || writing.write_until_gone(&asked))?;
        Ok(Days { shared, writer })
    }

    /// The day of the program `name`, which every run of it counts into; begun from what was kept
    /// of today, the first time the engine runs it.
    pub(crate) fn of(&self, name: &str) -> Arc<ProgramDay> {
        if let Some(day) = locked(&self.shared.programs).get(name) {
            return Arc::clone(day);
        }
        // Read outside the lock: reading the kept days reads a file.
        let today = day_of(SystemTime::now());
        let kept = self.shared.log.days(name).into_iter().next();
        let day = kept
            .filter(|kept| kept.start == today)
            .unwrap_or_else(|| AuditDay::starting(today));
        let mut programs = locked(&self.shared.programs);
        Arc::clone(
            programs
                .entry(name.to_owned())
                .or_insert_with(|| Arc::new(ProgramDay::beginning(day))),
        )
    }

    /// Asks the writer to keep the program `name`'s day now: the program ended.
    pub(crate) fn keep_now(&self, name: &str) {
        // A writer gone keeps nothing more; nothing is lost that it would have kept.
        let _ = self.writer.send(Ask::Keep(name.to_owned()));
    }

    /// Keeps every day that changed, through the writer, after what it was asked before;
    /// returns once they are kept. Writes files, so call it off the main thread.
    pub(crate) fn keep_all(&self) {
        let (done, kept) = mpsc::channel();
        if self.writer.send(Ask::KeepAll(done)).is_ok() {
            // A writer gone has kept everything a last time already.
            let _ = kept.recv();
        }
    }

    /// The days kept of the program `name`, newest first, today's as counted in memory now in
    /// place of the one kept, where the engine counts one. Reads files, so call it off a
    /// program's thread.
    pub(crate) fn days_of(&self, name: &str) -> Vec<AuditDay> {
        let mut days = self.shared.log.days(name);
        let counted = locked(&self.shared.programs).get(name).cloned();
        if let Some(today) = counted.map(|day| day.today()) {
            days.retain(|kept| kept.start != today.start);
            days.insert(0, today);
        }
        days
    }
}

impl Shared {
    /// Keeps each day that changed once a minute, and a program's day when asked, until every
    /// [`Days`] is gone; then keeps them all a last time.
    fn write_until_gone(&self, asked: &mpsc::Receiver<Ask>) {
        loop {
            match asked.recv_timeout(KEEP_EVERY) {
                Ok(Ask::Keep(name)) => self.keep(&name),
                Ok(Ask::KeepAll(done)) => {
                    self.keep_all();
                    // Whoever asked may have stopped waiting.
                    let _ = done.send(());
                }
                Err(RecvTimeoutError::Timeout) => self.keep_all(),
                Err(RecvTimeoutError::Disconnected) => {
                    self.keep_all();
                    return;
                }
            }
        }
    }

    fn keep(&self, name: &str) {
        let day = locked(&self.programs).get(name).cloned();
        if let Some(day) = day {
            self.write(name, &day);
        }
    }

    fn keep_all(&self) {
        let programs: Vec<(String, Arc<ProgramDay>)> = locked(&self.programs)
            .iter()
            .map(|(name, day)| (name.clone(), Arc::clone(day)))
            .collect();
        for (name, day) in programs {
            self.write(&name, &day);
        }
    }

    /// Writes what `day` has to keep, outside its lock; nothing, and what was counted let go,
    /// while its user keeps no record of the program.
    fn write(&self, name: &str, day: &ProgramDay) {
        if !day.keeps_record() {
            day.forget();
            return;
        }
        for kept in day.take_to_keep() {
            self.log.keep(name, &kept);
        }
    }
}

impl ProgramDay {
    /// A day that counts nowhere but in memory, for a program the engine keeps no days of.
    pub(crate) fn in_memory() -> Arc<ProgramDay> {
        Arc::new(ProgramDay::beginning(AuditDay::starting(day_of(
            SystemTime::now(),
        ))))
    }

    fn beginning(day: AuditDay) -> ProgramDay {
        ProgramDay {
            counted: Mutex::new(Counted {
                day,
                changed: false,
                ended: Vec::new(),
            }),
            grants: Mutex::default(),
            udp_datagrams: AtomicU64::new(0),
        }
    }

    /// A run of the program starts with `grants`, whose record setting the writer follows.
    pub(crate) fn follow(&self, grants: Arc<dyn ProgramGrants>) {
        *locked(&self.grants) = Some(grants);
    }

    /// Whether the user keeps a record of the program, as its latest run's grants say.
    fn keeps_record(&self) -> bool {
        locked(&self.grants)
            .as_ref()
            .is_none_or(|grants| grants.keeps_record())
    }

    /// Changes the day by `change`, given the time now, a day that ended set aside to be kept and
    /// a new one begun first.
    pub(crate) fn change(&self, change: impl FnOnce(&mut AuditDay, SystemTime)) {
        let now = SystemTime::now();
        let mut counted = locked(&self.counted);
        if day_of(now) != counted.day.start {
            self.fold_datagrams(&mut counted);
            let ended = std::mem::replace(&mut counted.day, AuditDay::starting(day_of(now)));
            if std::mem::take(&mut counted.changed) {
                counted.ended.push(ended);
            }
        }
        change(&mut counted.day, now);
        counted.changed = true;
    }

    /// Counts a UDP datagram, apart from the lock.
    pub(crate) fn count_datagram(&self) {
        self.udp_datagrams.fetch_add(1, Ordering::Relaxed);
    }

    /// Lets go of what was counted of today and not kept: the user keeps no record of the
    /// program now.
    pub(crate) fn forget(&self) {
        let mut counted = locked(&self.counted);
        counted.day = AuditDay::starting(counted.day.start);
        counted.changed = false;
        counted.ended.clear();
        self.udp_datagrams.store(0, Ordering::Relaxed);
    }

    /// Today, as counted so far.
    pub(crate) fn today(&self) -> AuditDay {
        let mut counted = locked(&self.counted);
        self.fold_datagrams(&mut counted);
        counted.day.clone()
    }

    /// The days to keep: those that ended, and today's where it changed since it was last kept.
    fn take_to_keep(&self) -> Vec<AuditDay> {
        let mut counted = locked(&self.counted);
        self.fold_datagrams(&mut counted);
        let mut to_keep = std::mem::take(&mut counted.ended);
        if std::mem::take(&mut counted.changed) {
            to_keep.push(counted.day.clone());
        }
        to_keep
    }

    fn fold_datagrams(&self, counted: &mut Counted) {
        let udp_datagrams = self.udp_datagrams.swap(0, Ordering::Relaxed);
        if udp_datagrams > 0 {
            counted.day.udp_datagrams += udp_datagrams;
            counted.changed = true;
        }
    }
}

/// The midnight, UTC, that begins the day `at` is in.
pub(crate) fn day_of(at: SystemTime) -> SystemTime {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    UNIX_EPOCH + Duration::from_secs(seconds - seconds % DAY)
}
