//! The programs' audit summaries, kept on this device only: one file per program and day,
//! `audit/<program>/<date>.postcard`, each written whole in place of the last, the days older than
//! [`DAYS_KEPT`] removed as a new one is kept, and all of a program's with the program.
//!
//! The invariant: a day is kept only for a program the store has a folder of, under the store's
//! lock, so a program removed is never kept again by a run of it that ends after.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::warn;
use waos_capabilities::{AuditDay, AuditLog, DAYS_KEPT};

use crate::files::write_whole;
use crate::store::Store;

/// The folder of the audit summaries, in the store's directory.
pub(crate) const AUDIT: &str = "audit";

/// What a day's file is named after its date.
const EXTENSION: &str = "postcard";

impl AuditLog for Store {
    fn keep(&self, program: &str, day: &AuditDay) {
        let Some(folder) = self.audit_folder_of(program) else {
            warn!(
                program,
                "a program's day is not kept: its name names no folder"
            );
            return;
        };
        let _changing = self.lock_changes();
        if !self.has_folder_of(program) {
            return;
        }
        if let Err(error) = keep_day(&folder, day) {
            warn!(program, %error, "a program's day is not kept");
        }
    }

    fn days(&self, program: &str) -> Vec<AuditDay> {
        let Some(folder) = self.audit_folder_of(program) else {
            return Vec::new();
        };
        let mut days: Vec<AuditDay> = day_files(&folder)
            .into_iter()
            .filter_map(|(_, path)| read_day(&path))
            .collect();
        days.sort_by_key(|day| std::cmp::Reverse(day.start));
        days.truncate(DAYS_KEPT);
        days
    }
}

impl Store {
    /// Where `program`'s days are kept; none for a name that is no one folder's.
    fn audit_folder_of(&self, program: &str) -> Option<PathBuf> {
        let one_name =
            !program.is_empty() && !matches!(program, "." | "..") && !program.contains(['/', '\\']);
        one_name.then(|| self.directory().join(AUDIT).join(program))
    }

    /// Removes every day kept of `program`; a program none were kept of has nothing to remove.
    pub(crate) fn forget_days(&self, program: &str) {
        let Some(folder) = self.audit_folder_of(program) else {
            return;
        };
        match fs::remove_dir_all(&folder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => warn!(program, %error, "a removed program's days are left"),
        }
    }
}

/// Writes `day` in `folder` in place of what was kept for it, and removes the days too old to
/// keep.
fn keep_day(folder: &Path, day: &AuditDay) -> std::io::Result<()> {
    fs::create_dir_all(folder)?;
    let bytes = postcard::to_stdvec(day).map_err(std::io::Error::other)?;
    write_whole(&folder.join(file_name(day.start)), &bytes)?;
    let oldest = day.start - Duration::from_secs(DAY * (DAYS_KEPT as u64 - 1));
    for (start, path) in day_files(folder) {
        if start < oldest {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// The day kept at `path`; none, with a warning, for a file that is not one.
fn read_day(path: &Path) -> Option<AuditDay> {
    let read = fs::read(path).ok()?;
    match postcard::from_bytes(&read) {
        Ok(day) => Some(day),
        Err(error) => {
            warn!(path = %path.display(), %error, "a kept day does not read");
            None
        }
    }
}

/// The days kept in `folder`, each by when it starts and where its file is.
fn day_files(folder: &Path) -> Vec<(SystemTime, PathBuf)> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let start = start_of(path.file_stem()?.to_str()?)?;
            (path.extension()? == EXTENSION).then_some((start, path))
        })
        .collect()
}

/// A day, in seconds.
const DAY: u64 = 24 * 60 * 60;

/// The name of the file of the day that starts at `start`: its date, `2026-11-15.postcard`.
fn file_name(start: SystemTime) -> String {
    let days = start
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / DAY;
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    format!("{year:04}-{month:02}-{day:02}.{EXTENSION}")
}

/// When the day `date`, `2026-11-15`, starts; none for text that is no date.
fn start_of(date: &str) -> Option<SystemTime> {
    let mut parts = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    let days = u64::try_from(days_from_civil(year, month, day)).ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(days * DAY))
}

/// The date of the day `days` after 1970-01-01, by Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The day after 1970-01-01 that `year`-`month`-`day` is, by Howard Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use waos_capabilities::Uses;

    use super::*;
    use crate::SourceProxy;

    fn day(days_since_epoch: u64) -> AuditDay {
        let mut day = AuditDay::starting(UNIX_EPOCH + Duration::from_secs(days_since_epoch * DAY));
        day.notifications = Uses {
            allowed: 1,
            ..Uses::default()
        };
        day
    }

    #[test]
    fn a_date_names_the_day_it_is_and_back() {
        let start = UNIX_EPOCH + Duration::from_secs(20_407 * DAY);
        assert_eq!(file_name(start), "2025-11-15.postcard");
        assert_eq!(start_of("2025-11-15"), Some(start));
        assert_eq!(start_of("1970-01-01"), Some(UNIX_EPOCH));
        assert_eq!(start_of("not a date"), None);
    }

    #[test]
    fn a_programs_last_seven_days_are_kept_newest_first_and_go_with_it() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path(), SourceProxy::None).unwrap();
        let id = "dev.waos.notes".parse().unwrap();
        store.folders(&id).unwrap();

        for days in 20_400..20_410 {
            store.keep("dev.waos.notes", &day(days));
        }
        let mut today = day(20_409);
        today.pastes = 3;
        store.keep("dev.waos.notes", &today);
        let kept = store.days("dev.waos.notes");
        assert_eq!(kept.len(), DAYS_KEPT);
        assert_eq!(
            kept[0], today,
            "the day kept last, in place of the one before"
        );
        assert!(kept.windows(2).all(|pair| pair[0].start > pair[1].start));
        let files = fs::read_dir(directory.path().join(AUDIT).join("dev.waos.notes"))
            .unwrap()
            .count();
        assert_eq!(files, DAYS_KEPT, "the older days are removed");

        store.forget_days("dev.waos.notes");
        assert!(
            store.days("dev.waos.notes").is_empty(),
            "forgotten with the program"
        );
        store.keep("dev.waos.never-run", &day(20_410));
        assert!(
            store.days("dev.waos.never-run").is_empty(),
            "a program the store has no folder of is kept nothing of"
        );
    }

    #[test]
    fn a_name_that_is_no_one_folders_keeps_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path(), SourceProxy::None).unwrap();
        store.keep("../elsewhere", &day(20_400));
        assert!(!directory.path().join("elsewhere").exists());
        assert!(store.days("../elsewhere").is_empty());
    }
}
