//! A program's report: what it did over the days its device keeps, laid out as Windows Settings
//! lays out an app's privacy and iOS's App Privacy Report an app's, ready for either kit to draw.
//! What WaOS refused, summed first; then each kind of access it used, with how often on each of
//! the last seven days, refusals apart, and when it last did; the kinds it did not use, in one
//! line; the domains it contacted, the most contacted first; and the questions it put, with the
//! answers.
//!
//! The invariant: the report says what the days say and no more, so a day's bar counts only what
//! that day counted of its kind.

use waos_activity::{AccessKind, Activity, Counts};
use wasdk::inspect::audit::Day;

use crate::clock;
use crate::logic::activity::activity_of;
use crate::logic::detail::Glyph;
use crate::logic::panels::{Cell, Column, Tabular, Tone};
use crate::words;

/// How many days a strip shows, the days the device keeps.
pub(crate) const DAYS: usize = 7;

/// A day, in seconds.
const DAY: i64 = 24 * 60 * 60;

/// A program's report.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Report {
    /// Each kind of access it used, in the order its user worries about them.
    pub(crate) used: Vec<Used>,
    /// The kinds it did not use.
    pub(crate) unused: Vec<AccessKind>,
    /// The domains it contacted, the most contacted first.
    pub(crate) domains: Vec<Contacted>,
    /// The addresses of this computer and the local network it reached.
    pub(crate) local_addresses: usize,
    pub(crate) questions: Vec<String>,
}

/// One kind of access it used, with how often on which day.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Used {
    pub(crate) kind: AccessKind,
    /// What it did, in short: `twice, 12 minutes`.
    pub(crate) said: String,
    /// Its uses on each of the strip's days, oldest first, today last.
    pub(crate) days: [Counts; DAYS],
    /// When it last used it, in seconds since the epoch; none where the days do not say.
    pub(crate) last: Option<i64>,
}

impl Used {
    /// Its uses over the strip's days.
    pub(crate) fn total(&self) -> Counts {
        self.days
            .iter()
            .fold(Counts::default(), |total, day| Counts {
                allowed: total.allowed + day.allowed,
                refused: total.refused + day.refused,
            })
    }
}

/// A domain contacted.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Contacted {
    pub(crate) domain: String,
    pub(crate) times: u32,
    pub(crate) refused: u32,
    pub(crate) last: Option<i64>,
}

impl Report {
    /// The report of `days`, against `today`, the start of today in seconds since the epoch.
    pub(crate) fn of(days: &[Day], today: i64) -> Report {
        let activity = activity_of(days);
        let mut used = Vec::new();
        let mut unused = Vec::new();
        for kind in AccessKind::ALL {
            match said(&activity, kind) {
                Some(said) => used.push(Used {
                    kind,
                    said,
                    days: days_of(days, kind, today),
                    last: last_use(days, kind),
                }),
                None => unused.push(kind),
            }
        }
        Report {
            used,
            unused,
            domains: contacted(days, &activity),
            local_addresses: activity.local_addresses.len(),
            questions: (activity.sections().into_iter())
                .find(|section| section.kind == AccessKind::Questions)
                .map(|section| section.lines)
                .unwrap_or_default(),
        }
    }

    /// The uses WaOS refused, of every kind.
    pub(crate) fn refused(&self) -> u32 {
        self.used.iter().map(|used| used.total().refused).sum()
    }

    /// Whether the days say nothing at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.used.is_empty()
    }
}

/// What `activity` did of `kind`, in short; none where it did nothing of it.
fn said(activity: &Activity, kind: AccessKind) -> Option<String> {
    if kind == AccessKind::Files {
        let opened = !activity.folders.is_empty() || activity.folder_refusals > 0;
        return opened.then(|| words::opened_files(&activity.folders, activity.folder_refusals));
    }
    (activity.sections().into_iter())
        .find(|section| section.kind == kind)
        .and_then(|section| section.lines.into_iter().next())
}

/// The uses of `kind` on each of the strip's days, oldest first.
fn days_of(days: &[Day], kind: AccessKind, today: i64) -> [Counts; DAYS] {
    let mut counted = [Counts::default(); DAYS];
    for day in days {
        let ago = (today - day.start.seconds).div_euclid(DAY);
        let Some(ago) = usize::try_from(ago).ok().filter(|ago| *ago < DAYS) else {
            continue;
        };
        let uses = uses_of(&activity_of(std::slice::from_ref(day)), kind);
        let slot = &mut counted[DAYS - 1 - ago];
        slot.allowed += uses.allowed;
        slot.refused += uses.refused;
    }
    counted
}

/// How often `activity` used `kind`, and how often WaOS refused it.
fn uses_of(activity: &Activity, kind: AccessKind) -> Counts {
    let sum = |counts: &mut dyn Iterator<Item = Counts>| {
        counts.fold(Counts::default(), |total, counts| Counts {
            allowed: total.allowed + counts.allowed,
            refused: total.refused + counts.refused,
        })
    };
    match kind {
        AccessKind::Camera => Counts {
            allowed: activity.camera.sessions,
            refused: 0,
        },
        AccessKind::Microphone => Counts {
            allowed: activity.microphone.sessions,
            refused: 0,
        },
        AccessKind::Network => sum(&mut (activity.sites.iter())
            .chain(&activity.local_addresses)
            .map(|named| named.uses)),
        AccessKind::Files => Counts {
            allowed: (activity.folders.iter())
                .map(|folder| folder.opened_to_read + folder.opened_to_write + folder.changes)
                .sum(),
            refused: activity.folder_refusals,
        },
        AccessKind::Clipboard => {
            let mut counts =
                sum(&mut [activity.clipboard_reads, activity.clipboard_writes].into_iter());
            counts.allowed += activity.pastes;
            counts
        }
        AccessKind::Machines => sum(&mut activity.commands.iter().map(|command| command.uses)),
        AccessKind::Links => sum(&mut activity.links.iter().map(|link| link.uses)),
        AccessKind::Notifications => activity.notifications,
        AccessKind::Questions => Counts {
            allowed: u32::try_from(activity.questions.len()).unwrap_or(u32::MAX),
            refused: 0,
        },
    }
}

/// When the days last counted a use of `kind`; none where they keep no time of it.
fn last_use(days: &[Day], kind: AccessKind) -> Option<i64> {
    let times: Vec<i64> = days
        .iter()
        .flat_map(|day| -> Vec<i64> {
            let at = match kind {
                AccessKind::Camera => day.camera.iter().map(|session| session.start).collect(),
                AccessKind::Microphone => {
                    day.microphone.iter().map(|session| session.start).collect()
                }
                AccessKind::Network => (day.domains.iter())
                    .chain(&day.local_addresses)
                    .filter_map(|named| named.uses.last)
                    .collect(),
                AccessKind::Files => Vec::new(),
                AccessKind::Clipboard => [day.clipboard_reads.last, day.clipboard_writes.last]
                    .into_iter()
                    .flatten()
                    .collect(),
                AccessKind::Machines => (day.commands.iter())
                    .filter_map(|command| command.uses.last)
                    .collect(),
                AccessKind::Links => day.links.iter().filter_map(|link| link.uses.last).collect(),
                AccessKind::Notifications => day.notifications.last.into_iter().collect(),
                AccessKind::Questions => day.questions.iter().map(|asked| asked.at).collect(),
            };
            at.into_iter().map(|at| at.seconds).collect()
        })
        .collect();
    times.into_iter().max()
}

/// The domains of `activity`, the most contacted first, each with when it was last contacted.
fn contacted(days: &[Day], activity: &Activity) -> Vec<Contacted> {
    let mut domains: Vec<Contacted> = activity
        .sites
        .iter()
        .map(|site| Contacted {
            domain: site.name.clone(),
            times: site.uses.allowed + site.uses.refused,
            refused: site.uses.refused,
            last: (days.iter())
                .flat_map(|day| &day.domains)
                .filter(|domain| domain.name == site.name)
                .filter_map(|domain| domain.uses.last.map(|at| at.seconds))
                .max(),
        })
        .collect();
    domains
        .sort_by(|left, right| (right.times.cmp(&left.times)).then(left.domain.cmp(&right.domain)));
    domains
}

impl Tabular for Contacted {
    fn columns() -> &'static [Column] {
        const COLUMNS: &[Column] = &[
            Column::rest(words::DOMAIN),
            Column::numeric(words::USES, 72.0),
            Column::numeric(words::REFUSED, 80.0),
            Column::numeric(words::LAST, 120.0),
        ];
        COLUMNS
    }

    fn cell_at(&self, index: usize) -> Cell {
        match index {
            0 => Cell::code(self.domain.clone()),
            1 => Cell::code(self.times.to_string()),
            2 if self.refused > 0 => Cell::code(self.refused.to_string()).toned(Tone::Refused),
            2 => Cell::plain("–").toned(Tone::Quiet),
            _ => {
                Cell::plain(self.last.map(clock::said_when).unwrap_or_default()).toned(Tone::Quiet)
            }
        }
    }

    fn leading(&self) -> Option<Glyph> {
        None
    }
}
