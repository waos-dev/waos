//! A program's report: what it did over the days its device keeps, laid out as iOS's App Privacy
//! Report and Android's Privacy dashboard lay theirs out, ready for either kit to draw. Each kind
//! of access it had, the camera and the microphone first, with a strip of the days it had it on
//! and when it last did; the domains it contacted, ranked by how often, each with its share of the
//! most contacted's; and the questions it put, with the answers.
//!
//! The invariant: the report says what the days say and no more, so a day is marked only where
//! that day counted a use of the kind.

use waos_activity::Activity;
use wasdk::inspect::audit::Day;

use crate::activity::activity_of;
use crate::words;

/// How many days a strip shows, the days the device keeps.
pub(crate) const DAYS: usize = 7;

/// A day, in seconds.
const DAY: i64 = 24 * 60 * 60;

/// A program's report.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Report {
    /// Each kind of access it had, the camera and the microphone first.
    pub(crate) access: Vec<Access>,
    /// The domains it contacted, the most contacted first.
    pub(crate) domains: Vec<Contacted>,
    /// The addresses of this computer and the local network it reached.
    pub(crate) local_addresses: usize,
    pub(crate) questions: Vec<String>,
}

/// One kind of access, with when.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Access {
    /// Its name, as the report's rows say it: Camera, Microphone, Files…
    pub(crate) kind: &'static str,
    /// What it did, in short: `twice, 12 minutes`.
    pub(crate) said: String,
    /// The days it had it on, oldest first, today last.
    pub(crate) days: [bool; DAYS],
    /// When it last had it, in seconds since the epoch; none where the days do not say.
    pub(crate) last: Option<i64>,
}

/// A domain contacted.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Contacted {
    pub(crate) domain: String,
    pub(crate) times: u32,
    pub(crate) refused: u32,
    /// Its share of the most contacted domain's times, from 0 to 1.
    pub(crate) share: f64,
    pub(crate) last: Option<i64>,
}

impl Report {
    /// The report of `days`, against `today`, the start of today in seconds since the epoch.
    pub(crate) fn of(days: &[Day], today: i64) -> Report {
        let activity = activity_of(days);
        let sections = activity.sections();
        let said = |kind: &str| {
            sections
                .iter()
                .find(|section| section.title == kind)
                .and_then(|section| section.lines.first().cloned())
        };
        let access = [
            "Camera",
            "Microphone",
            "Files",
            "Clipboard",
            "Machines",
            "Notifications",
        ]
        .into_iter()
        .filter_map(|kind| {
            let section = match kind {
                "Machines" => "Ran",
                kind => kind,
            };
            let said = match kind {
                "Files" => (!activity.folders.is_empty() || activity.folder_refusals > 0)
                    .then(|| words::opened_files(&activity.folders, activity.folder_refusals)),
                _ => said(section),
            };
            Some(Access {
                kind,
                said: said?,
                days: days_used(days, kind, today),
                last: last_use(days, kind),
            })
        })
        .collect();
        Report {
            access,
            domains: contacted(days, &activity),
            local_addresses: activity.local_addresses.len(),
            questions: said_all(&sections, "Asked"),
        }
    }

    /// Whether the days say nothing at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.access.is_empty()
            && self.domains.is_empty()
            && self.local_addresses == 0
            && self.questions.is_empty()
    }
}

/// Every line of the section `title` of `sections`.
fn said_all(sections: &[waos_activity::Section], title: &str) -> Vec<String> {
    sections
        .iter()
        .find(|section| section.title == title)
        .map(|section| section.lines.clone())
        .unwrap_or_default()
}

/// Which of the strip's days, oldest first, counted a use of `kind`.
fn days_used(days: &[Day], kind: &str, today: i64) -> [bool; DAYS] {
    let mut used = [false; DAYS];
    for day in days {
        let ago = (today - day.start.seconds).div_euclid(DAY);
        let Some(slot) = usize::try_from(ago).ok().filter(|ago| *ago < DAYS) else {
            continue;
        };
        let marks = activity_of(std::slice::from_ref(day)).marks();
        let mark = match kind {
            "Notifications" => {
                let posted = day.notifications.allowed + day.notifications.refused;
                used[DAYS - 1 - slot] |= posted > 0;
                continue;
            }
            kind => kind,
        };
        used[DAYS - 1 - slot] |= marks.contains(&mark);
    }
    used
}

/// When the days last counted a use of `kind`; none where they keep no time of it.
fn last_use(days: &[Day], kind: &str) -> Option<i64> {
    let times: Vec<i64> = days
        .iter()
        .flat_map(|day| -> Vec<i64> {
            match kind {
                "Camera" => day
                    .camera
                    .iter()
                    .map(|session| session.start.seconds)
                    .collect(),
                "Microphone" => (day.microphone.iter())
                    .map(|session| session.start.seconds)
                    .collect(),
                "Clipboard" => [day.clipboard_reads.last, day.clipboard_writes.last]
                    .into_iter()
                    .flatten()
                    .map(|at| at.seconds)
                    .collect(),
                "Machines" => (day.commands.iter())
                    .filter_map(|command| command.uses.last.map(|at| at.seconds))
                    .collect(),
                "Notifications" => day
                    .notifications
                    .last
                    .map(|at| at.seconds)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            }
        })
        .collect();
    times.into_iter().max()
}

/// The domains of `activity`, the most contacted first, each with when it was last contacted.
fn contacted(days: &[Day], activity: &Activity) -> Vec<Contacted> {
    let most = activity
        .sites
        .iter()
        .map(|site| site.uses.allowed + site.uses.refused)
        .max()
        .unwrap_or(0)
        .max(1);
    let mut domains: Vec<Contacted> = activity
        .sites
        .iter()
        .map(|site| {
            let times = site.uses.allowed + site.uses.refused;
            let last = (days.iter())
                .flat_map(|day| &day.domains)
                .filter(|domain| domain.name == site.name)
                .filter_map(|domain| domain.uses.last.map(|at| at.seconds))
                .max();
            Contacted {
                domain: site.name.clone(),
                times,
                refused: site.uses.refused,
                share: f64::from(times) / f64::from(most),
                last,
            }
        })
        .collect();
    domains.sort_by(|left, right| {
        right
            .times
            .cmp(&left.times)
            .then(left.domain.cmp(&right.domain))
    });
    domains
}
