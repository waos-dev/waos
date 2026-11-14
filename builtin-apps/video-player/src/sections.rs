//! What the library shows: all its videos, one folder's, or those left partway, Continue
//! Watching, the one left last first; and among them, those whose title holds what the search
//! says, in any case.

use std::time::Duration;

/// A part of the library the user chooses to see.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Section {
    All,
    /// The folder at this place among the library's folders.
    Folder(usize),
    /// The videos left partway, the one left last first.
    ContinueWatching,
}

/// A video as a section chooses among them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Listed<'a> {
    /// Where its folder is among the library's folders.
    pub(crate) folder: usize,
    pub(crate) title: &'a str,
    /// Where it was left, as the recent list keeps it, with how lately it was opened, from 0 for
    /// the newest; none where it is not among the recent.
    pub(crate) left: Option<(Duration, usize)>,
}

/// Where in `videos` the ones `section` shows are, in the order it shows them, among them only
/// those whose title holds `search`, spaces around it aside.
pub(crate) fn shown(section: Section, search: &str, videos: &[Listed]) -> Vec<usize> {
    let search = search.trim().to_lowercase();
    let mut shown: Vec<usize> = (0..videos.len())
        .filter(|index| is_in(section, &videos[*index]))
        .filter(|index| videos[*index].title.to_lowercase().contains(&search))
        .collect();
    if section == Section::ContinueWatching {
        shown.sort_by_key(|index| videos[*index].left.map(|(_, lately)| lately));
    }
    shown
}

/// Whether `section` holds `video`.
fn is_in(section: Section, video: &Listed) -> bool {
    match section {
        Section::All => true,
        Section::Folder(folder) => video.folder == folder,
        Section::ContinueWatching => video.left.is_some_and(|(at, _)| !at.is_zero()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(folder: usize, title: &str, left: Option<(u64, usize)>) -> Listed<'_> {
        Listed {
            folder,
            title,
            left: left.map(|(seconds, lately)| (Duration::from_secs(seconds), lately)),
        }
    }

    fn library() -> Vec<Listed<'static>> {
        vec![
            video(0, "Holiday", Some((30, 1))),
            video(0, "Birthday", None),
            video(1, "Lecture One", Some((0, 0))),
            video(1, "Lecture Two", Some((600, 2))),
        ]
    }

    #[test]
    fn a_section_shows_all_one_folders_or_those_left_partway_the_latest_first() {
        let videos = library();
        assert_eq!(shown(Section::All, "", &videos), [0, 1, 2, 3]);
        assert_eq!(shown(Section::Folder(1), "", &videos), [2, 3]);
        assert_eq!(
            shown(Section::ContinueWatching, "", &videos),
            [0, 3],
            "one left at its start is not partway"
        );
    }

    #[test]
    fn a_search_keeps_the_titles_holding_it_in_any_case() {
        let videos = library();
        assert_eq!(shown(Section::All, " lecture ", &videos), [2, 3]);
        assert_eq!(shown(Section::All, "DAY", &videos), [0, 1]);
        assert_eq!(shown(Section::ContinueWatching, "two", &videos), [3]);
        assert!(shown(Section::Folder(0), "lecture", &videos).is_empty());
    }
}
