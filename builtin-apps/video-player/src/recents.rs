//! The media opened lately, the newest first, each with where its user left it, kept in the
//! program's own `/data` so that a later run lists them and resumes each where it was left.
//!
//! The file holds a line per source: where it was left in milliseconds, a tab, and the source's
//! URL. A line that cannot be read is passed over, so a damaged file loses only its own lines.

use std::fs;
use std::time::Duration;

use crate::source::Source;

/// Where the list is kept between runs.
const RECENTS_FILE: &str = "/data/recents";

/// How many sources the list keeps.
const KEPT: usize = 10;

/// The sources opened lately, the newest first.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Recents {
    recents: Vec<Recent>,
}

/// A source opened lately, and where its user left it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Recent {
    pub(crate) source: Source,
    /// Where it opens next time; zero for its start.
    pub(crate) left_at: Duration,
}

/// The list as the last run kept it; empty at the first.
pub(crate) fn load() -> Recents {
    Recents::read(&fs::read_to_string(RECENTS_FILE).unwrap_or_default())
}

/// Keeps `recents` for the next run.
pub(crate) fn save(recents: &Recents) {
    if let Err(error) = fs::write(RECENTS_FILE, recents.text()) {
        eprintln!("the recent list is not kept: {error}");
    }
}

impl Recents {
    pub(crate) fn list(&self) -> &[Recent] {
        &self.recents
    }

    /// Where `source` was left, if it was opened lately.
    pub(crate) fn left_at(&self, source: &Source) -> Option<Duration> {
        self.find(source).map(|recent| recent.left_at)
    }

    /// Where `source` comes among the media opened lately, from 0 for the newest, with where it
    /// was left; none where it was not opened lately.
    pub(crate) fn recent(&self, source: &Source) -> Option<(usize, &Recent)> {
        self.recents
            .iter()
            .enumerate()
            .find(|(_, recent)| &recent.source == source)
    }

    /// `source` was opened now: first in the list, where it was left kept, the oldest past
    /// [`KEPT`] let go.
    pub(crate) fn opened(&mut self, source: &Source) {
        let left_at = self.left_at(source).unwrap_or_default();
        self.recents.retain(|recent| &recent.source != source);
        self.recents.insert(
            0,
            Recent {
                source: source.clone(),
                left_at,
            },
        );
        self.recents.truncate(KEPT);
    }

    /// `source` was left at `position`.
    pub(crate) fn left(&mut self, source: &Source, position: Duration) {
        if let Some(recent) = self
            .recents
            .iter_mut()
            .find(|recent| &recent.source == source)
        {
            recent.left_at = position;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.recents.clear();
    }

    fn find(&self, source: &Source) -> Option<&Recent> {
        self.recents.iter().find(|recent| &recent.source == source)
    }

    /// The list `text` holds, a line per source.
    fn read(text: &str) -> Recents {
        let recents = text.lines().filter_map(recent_in).take(KEPT).collect();
        Recents { recents }
    }

    /// The list as its file holds it.
    fn text(&self) -> String {
        self.recents
            .iter()
            .map(|recent| {
                let millis = recent.left_at.as_millis();
                format!("{millis}\t{}\n", recent.source.url())
            })
            .collect()
    }
}

/// The source `line` keeps, with where it was left.
fn recent_in(line: &str) -> Option<Recent> {
    let (millis, url) = line.split_once('\t')?;
    Some(Recent {
        source: Source::kept(url)?,
        left_at: Duration::from_millis(millis.parse().ok()?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(url: &str) -> Source {
        Source::kept(url).unwrap()
    }

    #[test]
    fn the_list_reads_back_as_it_was_written() {
        let mut recents = Recents::default();
        recents.opened(&source("https://example.com/a.m3u8"));
        recents.opened(&source("file:///picked/b.mov"));
        recents.left(
            &source("file:///picked/b.mov"),
            Duration::from_millis(61_500),
        );
        let text = recents.text();
        assert_eq!(
            text,
            "61500\tfile:///picked/b.mov\n0\thttps://example.com/a.m3u8\n"
        );
        assert_eq!(Recents::read(&text), recents);
    }

    #[test]
    fn opening_again_moves_a_source_first_and_keeps_where_it_was_left() {
        let a = source("https://example.com/a.m3u8");
        let b = source("https://example.com/b.m3u8");
        let mut recents = Recents::default();
        recents.opened(&a);
        recents.left(&a, Duration::from_secs(30));
        recents.opened(&b);
        recents.opened(&a);
        let order: Vec<_> = recents.list().iter().map(|recent| &recent.source).collect();
        assert_eq!(order, [&a, &b]);
        assert_eq!(recents.left_at(&a), Some(Duration::from_secs(30)));
        assert_eq!(recents.left_at(&b), Some(Duration::ZERO));
    }

    #[test]
    fn the_list_keeps_the_newest_ten() {
        let mut recents = Recents::default();
        for number in 0..12 {
            recents.opened(&source(&format!("https://example.com/{number}.mp4")));
        }
        assert_eq!(recents.list().len(), KEPT);
        assert_eq!(
            recents.list()[0].source,
            source("https://example.com/11.mp4")
        );
    }

    #[test]
    fn a_line_that_cannot_be_read_is_passed_over() {
        let recents = Recents::read(
            "12\tnot a url\nabc\thttps://example.com/a\n5000\thttps://example.com/b\nno tab\n",
        );
        assert_eq!(
            recents.list(),
            [Recent {
                source: source("https://example.com/b"),
                left_at: Duration::from_secs(5),
            }]
        );
    }
}
