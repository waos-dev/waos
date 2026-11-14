//! The library: the videos in the folders the user added, each with what the system read of it,
//! and what the user chose to see of them, a section and a search. A folder's videos are found
//! as it is added and at the start (`finding`); each video's length and title are read through
//! Waft's media assets, or taken from the cache where it kept them (`facts_cache`), and its
//! thumbnail is decoded from where it was kept (`thumbnail_cache`), or read from the video
//! (`facts`) and kept, one video at a time, in the order they are listed, so a large library fills
//! in from the top while it is used. A thumbnail is an image the app draws as any other.
//!
//! The invariant: a video is read once a run, and what is read of it is kept only while it is
//! still among the library's videos, so a folder taken away while its videos are read leaves
//! nothing of them behind.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use waft_embedder::{Image, MediaAsset, PickedFile, VideoFrame};
use waft_foundation::{App, AsyncApp, Entity, Handle};
use waft_widgets::TextEditingController;

use crate::facts::{Facts, facts_of, thumbnail_of};
use crate::facts_cache::{self, FactsCache, stamp_of};
use crate::finding::videos_in;
use crate::folders::{self, Folders};
use crate::recents::Recents;
use crate::sections::{self, Listed, Section};
use crate::source::Source;
use crate::thumbnail_cache;

/// The library's videos, what was read of them, and what the user sees of them.
pub(crate) struct Library {
    folders: Folders,
    /// The videos in each folder, in the folders' order.
    videos: Vec<Vec<Video>>,
    /// The lengths and titles read this run, by their videos' URLs.
    facts: HashMap<String, Facts>,
    /// Those read before, as the cache kept them.
    cached: FactsCache,
    /// The thumbnails read this run, by their videos' URLs.
    thumbnails: HashMap<String, Image>,
    /// The videos whose facts and thumbnails are yet to read, in order.
    unread: VecDeque<Video>,
    /// Whether a task reads them now.
    reading: bool,
    section: Section,
    /// The search field's text, which the field keeps across the library's screens.
    search_field: Handle<TextEditingController>,
}

/// A video found in one of the library's folders.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Video {
    pub(crate) source: Source,
    /// What tells its file changed; none where the system does not say, and then its facts are
    /// not cached.
    stamp: Option<String>,
}

/// A video as its tile shows it.
#[derive(Clone)]
pub(crate) struct Tile {
    pub(crate) source: Source,
    /// Its tags' title, else its file's name.
    pub(crate) title: String,
    /// None until read, and where the system could not read it.
    pub(crate) duration: Option<Duration>,
    /// None until read, and where the system has no picture of it.
    pub(crate) thumbnail: Option<Image>,
    /// How far it was watched, from 0 to 1; none where it was not, or its length is unknown.
    pub(crate) watched: Option<f64>,
}

impl Library {
    /// The library of the folders the last run kept, their videos found, with the facts kept of
    /// them, each read again where it was not.
    pub(crate) fn new(app: &mut App) -> Entity<Library> {
        let folders = folders::load();
        let videos: Vec<Vec<Video>> = folders
            .list()
            .iter()
            .map(|folder| found_in(folder))
            .collect();
        let search_field = TextEditingController::new(app);
        let library = app.new_entity(|_cx| Library {
            unread: videos.iter().flatten().cloned().collect(),
            folders,
            videos,
            facts: HashMap::new(),
            cached: facts_cache::load(),
            thumbnails: HashMap::new(),
            reading: false,
            section: Section::All,
            search_field,
        });
        read_unread(&library, app);
        library
    }

    pub(crate) fn folders(&self) -> &[PathBuf] {
        self.folders.list()
    }

    pub(crate) fn section(&self) -> Section {
        self.section
    }

    pub(crate) fn search_field(&self) -> Handle<TextEditingController> {
        self.search_field
    }

    /// The tiles of the section chosen, among them those whose title holds `search`, each
    /// watched as far as `recents` says it was left.
    pub(crate) fn tiles(&self, search: &str, recents: &Recents) -> Vec<Tile> {
        let all: Vec<(usize, &Video)> = self
            .videos
            .iter()
            .enumerate()
            .flat_map(|(folder, videos)| videos.iter().map(move |video| (folder, video)))
            .collect();
        let titles: Vec<String> = all.iter().map(|(_, video)| self.title_of(video)).collect();
        let listed: Vec<Listed> = all
            .iter()
            .zip(&titles)
            .map(|((folder, video), title)| Listed {
                folder: *folder,
                title,
                left: recents
                    .recent(&video.source)
                    .map(|(lately, recent)| (recent.left_at, lately)),
            })
            .collect();
        sections::shown(self.section, search, &listed)
            .into_iter()
            .map(|index| self.tile_of(all[index].1, &titles[index], &listed[index]))
            .collect()
    }

    fn tile_of(&self, video: &Video, title: &str, listed: &Listed) -> Tile {
        let duration = self.facts_of(video).and_then(|facts| facts.duration);
        let watched = listed
            .left
            .zip(duration)
            .filter(|((at, _), duration)| !at.is_zero() && !duration.is_zero())
            .map(|((at, _), duration)| (at.as_secs_f64() / duration.as_secs_f64()).min(1.0));
        Tile {
            source: video.source.clone(),
            title: title.to_owned(),
            duration,
            thumbnail: self.thumbnails.get(video.source.url()).cloned(),
            watched,
        }
    }

    /// `video`'s title: its tags', once read, else its file's name.
    fn title_of(&self, video: &Video) -> String {
        self.facts_of(video)
            .and_then(|facts| facts.title.clone())
            .unwrap_or_else(|| video.source.name())
    }

    /// The facts read of `video`: this run, or before, where its file is unchanged since.
    fn facts_of(&self, video: &Video) -> Option<&Facts> {
        let url = video.source.url();
        self.facts
            .get(url)
            .or_else(|| self.cached.get(url, video.stamp.as_deref()?))
    }

    /// Whether `video` is still among the library's.
    fn holds(&self, video: &Video) -> bool {
        self.videos.iter().flatten().any(|held| held == video)
    }

    /// Keeps what was read of `video`, if it is still the library's.
    fn keep(&mut self, video: &Video, facts: Facts, thumbnail: Option<Image>) {
        if !self.holds(video) {
            return;
        }
        if let Some(stamp) = &video.stamp {
            self.cached.put(video.source.url(), stamp, facts.clone());
            facts_cache::save(&self.cached);
        }
        self.facts.insert(video.source.url().to_owned(), facts);
        if let Some(thumbnail) = thumbnail {
            self.thumbnails
                .insert(video.source.url().to_owned(), thumbnail);
        }
    }
}

/// Adds `folder` to the library, the user's pick, with its videos, which are read next; one the
/// library has already stays as it is. It is chosen, to show what it holds.
pub(crate) fn add_folder(library: &Entity<Library>, app: &mut App, folder: &Path) {
    let added = library.update(app, |library, cx| {
        if !library.folders.add(folder) {
            return false;
        }
        folders::save(&library.folders);
        let found = found_in(folder);
        library.unread.extend(found.iter().cloned());
        library.videos.push(found);
        library.section = Section::Folder(library.videos.len() - 1);
        cx.notify();
        true
    });
    if added {
        read_unread(library, app);
    }
}

/// Takes the folder at `index` out of the library, with its videos; the files stay where they
/// are, and the pick among the program's files until the user removes it in WaOS.
pub(crate) fn remove_folder(library: &Entity<Library>, app: &mut App, index: usize) {
    library.update(app, |library, cx| {
        if index >= library.videos.len() {
            return;
        }
        library.folders.remove(index);
        folders::save(&library.folders);
        let gone = library.videos.remove(index);
        library.unread.retain(|video| !gone.contains(video));
        for video in &gone {
            library.thumbnails.remove(video.source.url());
        }
        library.section = section_after_removing(library.section, index);
        cx.notify();
    });
}

/// The search field's text changed: the tiles are built again for it.
pub(crate) fn search_changed(library: &Entity<Library>, app: &mut App) {
    library.update(app, |_library, cx| cx.notify());
}

/// Shows `section` from now on.
pub(crate) fn choose(library: &Entity<Library>, app: &mut App, section: Section) {
    library.update(app, |library, cx| {
        library.section = section;
        cx.notify();
    });
}

/// The section chosen once the folder at `removed` is taken away: All in place of that folder's,
/// and the same folder where it came later, one place sooner.
fn section_after_removing(section: Section, removed: usize) -> Section {
    match section {
        Section::Folder(folder) if folder == removed => Section::All,
        Section::Folder(folder) if folder > removed => Section::Folder(folder - 1),
        section => section,
    }
}

/// The videos found in `folder`.
fn found_in(folder: &Path) -> Vec<Video> {
    videos_in(folder)
        .into_iter()
        .filter_map(|path| {
            let source = Source::picked(&PickedFile::Path(path.clone()))?;
            Some(Video {
                source,
                stamp: stamp_of(&path),
            })
        })
        .collect()
}

/// Reads the library's unread videos one after another in a task of their own, unless one runs
/// already, which reads those added meanwhile too; nothing where the system has no reading of
/// media.
fn read_unread(library: &Entity<Library>, app: &mut App) {
    if app.platform().media_assets().is_none() || library.read(app).reading {
        return;
    }
    library.update(app, |library, _cx| library.reading = true);
    let library = library.clone();
    app.spawn(async move |cx| {
        while let Some(video) =
            cx.update(|app| library.update(app, |library, _cx| library.unread.pop_front()))
        {
            if !read(cx, &library, &video).await {
                break;
            }
        }
        cx.update(|app| library.update(app, |library, _cx| library.reading = false));
    });
}

/// Reads `video`'s length, title and thumbnail where none is kept, and keeps them in `library`,
/// and its thumbnail on disk once it is shown; false where the system no longer reads media.
async fn read(cx: &AsyncApp, library: &Entity<Library>, video: &Video) -> bool {
    let known = cx.update(|app| library.read(app).facts_of(video).cloned());
    let kept_at = video
        .stamp
        .as_deref()
        .map(|stamp| thumbnail_cache::path_of(video.source.url(), stamp));
    let kept = match &kept_at {
        Some(path) => thumbnail_cache::kept(cx, path.clone()).await,
        None => None,
    };
    let (facts, thumbnail, to_keep) = match (known, kept) {
        (Some(facts), Some(thumbnail)) => (facts, Some(thumbnail), None),
        (known, kept) => {
            let asset = cx.update(|app| {
                let platform = app.platform();
                Some(platform.media_assets()?.open(video.source.url()))
            });
            let Some(asset) = asset else {
                return false;
            };
            let facts = match known {
                Some(facts) => facts,
                None => facts_of(asset.as_ref()).await,
            };
            let (thumbnail, to_keep) = match kept {
                Some(thumbnail) => (Some(thumbnail), None),
                None => read_thumbnail(cx, asset.as_ref(), facts.duration).await,
            };
            (facts, thumbnail, to_keep)
        }
    };
    cx.update(|app| {
        library.update(app, |library, cx| {
            library.keep(video, facts, thumbnail);
            cx.notify();
        });
    });
    if let (Some(frame), Some(path)) = (to_keep, kept_at) {
        thumbnail_cache::keep(cx, frame, path).await;
    }
    true
}

/// `asset`'s thumbnail as an image of the frame the system read, with the frame, to keep; none
/// where the system has no picture of it.
async fn read_thumbnail(
    cx: &AsyncApp,
    asset: &dyn MediaAsset,
    duration: Option<Duration>,
) -> (Option<Image>, Option<Arc<dyn VideoFrame>>) {
    let Some(frame) = thumbnail_of(asset, duration).await else {
        return (None, None);
    };
    let image = cx.update(|app| app.platform().import_video_frame(frame.as_ref()));
    (image, Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_taken_away_leaves_the_section_on_what_remains() {
        assert_eq!(section_after_removing(Section::Folder(1), 1), Section::All);
        assert_eq!(
            section_after_removing(Section::Folder(2), 1),
            Section::Folder(1)
        );
        assert_eq!(
            section_after_removing(Section::Folder(0), 1),
            Section::Folder(0)
        );
        assert_eq!(
            section_after_removing(Section::ContinueWatching, 0),
            Section::ContinueWatching
        );
    }
}
