//! Which apps their user starts most lately, for the home's first row: each start the engine
//! counted over the days it keeps of an app (`AuditDay::starts`), a start counting less the longer
//! ago its day, so the row settles on the apps in use now and an app left alone drops out within
//! the days kept, as Windows' Start lists its most used apps by their launches.
//!
//! The invariant: an app never started in the days kept is never listed, and neither is one whose
//! user keeps no record of it, which has no days.

use std::time::{Duration, SystemTime};

use waos::{AuditDay, DAYS_KEPT};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// How much an app was started lately, from the `days` kept of it as of `now`: each start counts
/// once for every day kept that its own day still is, today's the most.
pub(crate) fn starts_lately(days: &[AuditDay], now: SystemTime) -> u32 {
    days.iter()
        .map(|day| day.starts.saturating_mul(days_still_kept(day.start, now)))
        .sum()
}

/// For how many more days a day that began at `start` is kept, as of `now`: all of them for
/// today's, one for the oldest kept, none for one older or yet to come.
fn days_still_kept(start: SystemTime, now: SystemTime) -> u32 {
    let Ok(since) = now.duration_since(start) else {
        return 0;
    };
    let days_ago = (since.as_secs() / DAY.as_secs()) as usize;
    DAYS_KEPT.saturating_sub(days_ago) as u32
}

/// The apps of `started_lately` started at all, the most started first, those started alike in
/// the order given.
pub(crate) fn most_started<Id: Clone>(started_lately: &[(Id, u32)]) -> Vec<Id> {
    let mut started: Vec<&(Id, u32)> = started_lately
        .iter()
        .filter(|(_, starts)| *starts > 0)
        .collect();
    started.sort_by_key(|(_, starts)| std::cmp::Reverse(*starts));
    started.into_iter().map(|(id, _)| id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(days_ago: u32, starts: u32, now: SystemTime) -> AuditDay {
        let mut day = AuditDay::starting(now - DAY * days_ago);
        day.starts = starts;
        day
    }

    #[test]
    fn a_start_counts_less_the_longer_ago_its_day_and_nothing_past_the_days_kept() {
        let now = SystemTime::UNIX_EPOCH + DAY * 1000;
        assert_eq!(starts_lately(&[day(0, 1, now)], now), 7);
        assert_eq!(starts_lately(&[day(6, 1, now)], now), 1);
        assert_eq!(starts_lately(&[day(7, 5, now)], now), 0);
        assert_eq!(
            starts_lately(&[day(0, 2, now), day(3, 3, now)], now),
            14 + 12
        );
    }

    #[test]
    fn the_most_started_come_first_and_one_never_started_is_left_out() {
        let started = [("notes", 3), ("camera", 0), ("zed", 9), ("video", 3)];
        assert_eq!(most_started(&started), ["zed", "notes", "video"]);
    }
}
