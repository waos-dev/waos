//! The apps' days as files in WaOS's store directory ([`ActivityFiles`]): one file per app and
//! day, `audit/<app>/<date>.postcard`, each written whole in place of the last.
//!
//! ```text
//! audit/
//!   dev.example.triangle/
//!     2026-11-15.postcard
//! ```
//!
//! The invariant a later editor will break: these are the files and the place WaOS has always kept
//! the days in, which people have; a change of a name or of the format loses what was kept.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::warn;
use waos_capabilities::AuditDay;
use waos_package::ProgramId;

use crate::activity::ActivityStore;
use crate::files::write_whole;

/// The folder of the days, in the directory.
const AUDIT: &str = "audit";

/// What a day's file is named after its date.
const EXTENSION: &str = "postcard";

/// The apps' days as files in a directory, WaOS's own.
pub struct ActivityFiles {
    folder: PathBuf,
}

impl ActivityFiles {
    /// The days kept in `directory`.
    pub fn new(directory: &Path) -> ActivityFiles {
        ActivityFiles {
            folder: directory.join(AUDIT),
        }
    }

    fn folder_of(&self, app: &ProgramId) -> PathBuf {
        self.folder.join(app.as_str())
    }
}

impl ActivityStore for ActivityFiles {
    fn days(&self, app: &ProgramId) -> io::Result<Vec<AuditDay>> {
        Ok(day_files(&self.folder_of(app))
            .into_iter()
            .filter_map(|(_, path)| read_day(&path))
            .collect())
    }

    fn keep(&self, app: &ProgramId, day: &AuditDay) -> io::Result<()> {
        let folder = self.folder_of(app);
        fs::create_dir_all(&folder)?;
        let bytes = postcard::to_stdvec(day).map_err(io::Error::other)?;
        write_whole(&folder.join(file_name(day.start)), &bytes)
    }

    fn remove(&self, app: &ProgramId, start: SystemTime) -> io::Result<()> {
        let path = self.folder_of(app).join(file_name(start));
        match fs::remove_file(path) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn forget(&self, app: &ProgramId) -> io::Result<()> {
        match fs::remove_dir_all(self.folder_of(app)) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// The apps with a day kept, by their folders' names; a folder that names no app is none.
    fn apps(&self) -> io::Result<Vec<ProgramId>> {
        let folders = match fs::read_dir(&self.folder) {
            Ok(folders) => folders,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        Ok(folders
            .flatten()
            .filter(|folder| !day_files(&folder.path()).is_empty())
            .filter_map(|folder| folder.file_name().to_str()?.parse().ok())
            .collect())
    }
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

/// A day, in seconds.
const DAY: u64 = 24 * 60 * 60;

#[cfg(test)]
mod tests {
    use waos_capabilities::Uses;

    use super::*;

    #[test]
    fn a_date_names_the_day_it_is_and_back() {
        let start = UNIX_EPOCH + Duration::from_secs(20_407 * DAY);
        assert_eq!(file_name(start), "2025-11-15.postcard");
        assert_eq!(start_of("2025-11-15"), Some(start));
        assert_eq!(start_of("1970-01-01"), Some(UNIX_EPOCH));
        assert_eq!(start_of("not a date"), None);
    }

    #[test]
    fn a_day_reads_back_and_its_app_is_listed_until_forgotten() {
        let directory = tempfile::tempdir().unwrap();
        let files = ActivityFiles::new(directory.path());
        let notes: ProgramId = "dev.waos.notes".parse().unwrap();
        let mut day = AuditDay::starting(UNIX_EPOCH + Duration::from_secs(20_407 * DAY));
        day.notifications = Uses {
            allowed: 1,
            ..Uses::default()
        };
        files.keep(&notes, &day).unwrap();
        let kept = directory
            .path()
            .join("audit/dev.waos.notes/2025-11-15.postcard");
        assert!(kept.is_file(), "where WaOS always kept it");
        assert_eq!(files.days(&notes).unwrap(), [day.clone()]);
        assert_eq!(files.apps().unwrap(), std::slice::from_ref(&notes));
        files.remove(&notes, day.start).unwrap();
        assert!(files.days(&notes).unwrap().is_empty());
        files.keep(&notes, &day).unwrap();
        files.forget(&notes).unwrap();
        assert!(files.apps().unwrap().is_empty());
    }
}
