//! The facts the library read of its videos, kept in the program's own `/cache` so that a later
//! run shows each tile's length and title at once, before the system has read anything. A video's
//! facts are kept with its file's size and the time it was changed, and read back only while the
//! file still has them.
//!
//! The file holds a line per video: its URL, a tab, its stamp, a tab, its length in milliseconds
//! or `-`, a tab, and its title, empty for none. A line that cannot be read is passed over.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::facts::Facts;

/// Where the facts are kept between runs.
const CACHE_FILE: &str = "/cache/videos";

/// The facts read of the library's videos, by each video's URL.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct FactsCache {
    kept: HashMap<String, Kept>,
}

/// One video's facts, with the stamp of its file when they were read.
#[derive(Clone, Debug, PartialEq)]
struct Kept {
    stamp: String,
    facts: Facts,
}

/// The facts as the last run kept them; none at the first.
pub(crate) fn load() -> FactsCache {
    FactsCache::read(&fs::read_to_string(CACHE_FILE).unwrap_or_default())
}

/// Keeps `cache` for the next run.
pub(crate) fn save(cache: &FactsCache) {
    if let Err(error) = fs::write(CACHE_FILE, cache.text()) {
        eprintln!("the library's facts are not kept: {error}");
    }
}

/// What tells a file changed: its size and when it was last changed, as `size-seconds`; none
/// where the system does not say.
pub(crate) fn stamp_of(file: &Path) -> Option<String> {
    let metadata = fs::metadata(file).ok()?;
    let changed = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}-{}", metadata.len(), changed.as_secs()))
}

impl FactsCache {
    /// The facts of the video at `url`, kept while its file had `stamp`.
    pub(crate) fn get(&self, url: &str, stamp: &str) -> Option<&Facts> {
        let kept = self.kept.get(url)?;
        (kept.stamp == stamp).then_some(&kept.facts)
    }

    /// Keeps `facts` of the video at `url`, whose file has `stamp`.
    pub(crate) fn put(&mut self, url: &str, stamp: &str, facts: Facts) {
        let kept = Kept {
            stamp: stamp.to_owned(),
            facts,
        };
        self.kept.insert(url.to_owned(), kept);
    }

    /// The cache `text` holds, a line per video.
    fn read(text: &str) -> FactsCache {
        let kept = text.lines().filter_map(kept_in).collect();
        FactsCache { kept }
    }

    /// The cache as its file holds it, in the order of the URLs.
    fn text(&self) -> String {
        let mut lines: Vec<String> = self
            .kept
            .iter()
            .map(|(url, kept)| {
                let duration = kept
                    .facts
                    .duration
                    .map_or("-".to_owned(), |duration| duration.as_millis().to_string());
                let title = kept.facts.title.as_deref().unwrap_or_default();
                format!("{url}\t{}\t{duration}\t{title}\n", kept.stamp)
            })
            .collect();
        lines.sort();
        lines.concat()
    }
}

/// The URL and the facts `line` keeps.
fn kept_in(line: &str) -> Option<(String, Kept)> {
    let mut fields = line.splitn(4, '\t');
    let (url, stamp, duration, title) = (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    );
    let duration = match duration {
        "-" => None,
        millis => Some(Duration::from_millis(millis.parse().ok()?)),
    };
    let facts = Facts {
        duration,
        title: (!title.is_empty()).then(|| title.to_owned()),
    };
    let kept = Kept {
        stamp: stamp.to_owned(),
        facts,
    };
    Some((url.to_owned(), kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_reads_back_as_it_was_written() {
        let mut cache = FactsCache::default();
        let holiday = Facts {
            duration: Some(Duration::from_millis(61_500)),
            title: Some("Holiday\twith a tab".to_owned()),
        };
        cache.put(
            "file:///films/holiday.mov",
            "1342-1700000000",
            holiday.clone(),
        );
        cache.put("file:///films/raw.mp4", "9-1", Facts::default());
        let text = cache.text();
        assert_eq!(
            text,
            "file:///films/holiday.mov\t1342-1700000000\t61500\tHoliday\twith a tab\n\
             file:///films/raw.mp4\t9-1\t-\t\n"
        );
        assert_eq!(FactsCache::read(&text), cache);
        assert_eq!(
            cache.get("file:///films/holiday.mov", "1342-1700000000"),
            Some(&holiday)
        );
    }

    #[test]
    fn a_video_whose_file_changed_has_no_facts_kept() {
        let mut cache = FactsCache::default();
        cache.put("file:///films/a.mp4", "10-1", Facts::default());
        assert_eq!(cache.get("file:///films/a.mp4", "11-2"), None);
        assert_eq!(cache.get("file:///films/b.mp4", "10-1"), None);
    }

    #[test]
    fn a_line_that_cannot_be_read_is_passed_over() {
        let cache = FactsCache::read("no tabs\nfile:///a\t1-1\tsoon\t\nfile:///b\t1-1\t5000\tB\n");
        assert_eq!(cache.kept.len(), 1);
        assert_eq!(
            cache
                .get("file:///b", "1-1")
                .and_then(|facts| facts.duration),
            Some(Duration::from_secs(5))
        );
    }
}
