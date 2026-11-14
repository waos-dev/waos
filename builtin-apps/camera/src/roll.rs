//! The roll: the photos the app keeps, as the window shows them, the newest first, each with when
//! it was taken and its thumbnail once the system has decoded one. The photos are the library's files, in the app's
//! own `/data/photos`; a thumbnail is decoded small from its photo, one after another as the app
//! starts and as each photo is kept.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use waft::ui::Image;
use waft::{App, Entity, Size};

use crate::library::{Library, taken_at};
use crate::pictures::picture_of;

/// Where the photos are kept between runs.
const PHOTOS: &str = "/data/photos";

/// The box a thumbnail is decoded to fit, in pixels: the strip's tiles on a dense screen.
const THUMBNAIL: Size = Size::new(320.0, 320.0);

/// A photo the app keeps.
pub(crate) struct RollPhoto {
    pub(crate) path: PathBuf,
    /// When it was taken; none where its file cannot be read.
    pub(crate) taken: Option<SystemTime>,
    /// None until the system has decoded it, and for a file it cannot read.
    pub(crate) thumbnail: Option<Image>,
}

pub(crate) struct Roll {
    library: Library,
    /// The newest first.
    photos: Vec<RollPhoto>,
}

impl Roll {
    /// The roll of the photos kept in the app's data, their thumbnails on the way.
    pub(crate) fn open(app: &mut App) -> Entity<Roll> {
        let library = Library::at(PHOTOS);
        let kept = library.photos();
        let photos = kept.iter().cloned().map(without_thumbnail).collect();
        let roll = app.new_entity(|_cx| Roll { library, photos });
        load_thumbnails(app, &roll, kept);
        roll
    }

    pub(crate) fn photos(&self) -> &[RollPhoto] {
        &self.photos
    }

    pub(crate) fn newest(&self) -> Option<&RollPhoto> {
        self.photos.first()
    }

    /// Where the photo at `path` is among the photos, the newest at 0.
    pub(crate) fn position(&self, path: &Path) -> Option<usize> {
        self.photos.iter().position(|photo| photo.path == path)
    }
}

/// Keeps `encoded`, a new photo in the format files ending in `extension` hold, as the roll's
/// newest.
pub(crate) fn keep(
    app: &mut App,
    roll: &Entity<Roll>,
    encoded: &[u8],
    extension: &str,
) -> io::Result<()> {
    let path = roll.update(app, |roll, cx| {
        let path = roll.library.keep(encoded, extension)?;
        roll.photos.insert(0, without_thumbnail(path.clone()));
        cx.notify();
        io::Result::Ok(path)
    })?;
    load_thumbnails(app, roll, vec![path]);
    Ok(())
}

/// Deletes the photo at `path`, for good.
pub(crate) fn delete(app: &mut App, roll: &Entity<Roll>, path: &Path) -> io::Result<()> {
    roll.update(app, |roll, cx| {
        roll.library.delete(path)?;
        roll.photos.retain(|photo| photo.path != path);
        cx.notify();
        Ok(())
    })
}

fn without_thumbnail(path: PathBuf) -> RollPhoto {
    RollPhoto {
        taken: taken_at(&path),
        path,
        thumbnail: None,
    }
}

/// Decodes the thumbnail of each photo at `paths`, in order, giving the roll each as it comes.
fn load_thumbnails(app: &mut App, roll: &Entity<Roll>, paths: Vec<PathBuf>) {
    let roll = roll.clone();
    app.spawn(async move |cx| {
        for path in paths {
            if let Some(thumbnail) = picture_of(cx, &path, Some(THUMBNAIL)).await {
                cx.update(|app| set_thumbnail(app, &roll, &path, thumbnail));
            }
        }
    });
}

/// Gives the photo at `path` its thumbnail, where the roll still has the photo.
fn set_thumbnail(app: &mut App, roll: &Entity<Roll>, path: &Path, thumbnail: Image) {
    roll.update(app, |roll, cx| {
        if let Some(photo) = roll.photos.iter_mut().find(|photo| photo.path == path) {
            photo.thumbnail = Some(thumbnail);
            cx.notify();
        }
    });
}
