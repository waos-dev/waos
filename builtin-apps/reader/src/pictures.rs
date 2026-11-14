//! The pictures the lists and the reading page show: each fetched once from its address, or read
//! from where it was kept ([`crate::picture_cache`]), decoded by the system's image codecs at the
//! size it is shown at, and uploaded as an image to draw. A picture is asked for by the widget
//! that shows it, as it comes into view, and is an entity of its own, so only that widget is
//! built again when it arrives.
//!
//! The invariant a later editor will break: no more than [`MOST_AT_ONCE`] pictures are fetched
//! and decoded at a time, the one asked for last first, and no more decoded pictures are held
//! than each size's `most_held`, so a long list costs neither its sites a burst of requests nor
//! the program its memory.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use url::Url;
use waft::ui::{Image, Size};
use waft::{App, AsyncApp, Entity};

use crate::http::{self, Answer, Validators};
use crate::picture_cache;
use crate::reader_app::ReaderApp;

/// The most pictures fetched and decoded at a time.
const MOST_AT_ONCE: usize = 4;

/// The largest picture fetched, in bytes.
const LARGEST_PICTURE: usize = 8 * 1024 * 1024;

/// What a picture's request says it accepts.
const ACCEPTED: &str = "image/*";

/// How large a picture is decoded: for a list's row, or for the reading page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PictureSize {
    Thumbnail,
    Full,
}

impl PictureSize {
    /// The square a picture is scaled down to fit when decoded, in pixels on a side.
    fn fits_within(self) -> f64 {
        match self {
            PictureSize::Thumbnail => 400.0,
            PictureSize::Full => 1600.0,
        }
    }

    /// The most decoded pictures of this size held.
    fn most_held(self) -> usize {
        match self {
            PictureSize::Thumbnail => 60,
            PictureSize::Full => 8,
        }
    }
}

/// A picture, as far as it has come.
#[derive(Clone)]
pub(crate) enum Picture {
    Loading,
    Ready(Image),
    /// It could not be fetched or decoded.
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PictureKey {
    address: String,
    size: PictureSize,
}

#[derive(Default)]
pub(crate) struct Pictures {
    held: HashMap<PictureKey, Entity<Picture>>,
    /// The pictures held, the one asked for longest ago first.
    asked: VecDeque<PictureKey>,
    /// The pictures yet to load, the one asked for last at the end.
    waiting: Vec<(PictureKey, Entity<Picture>)>,
    loading: usize,
}

impl Pictures {
    /// The picture `key` names where it is held, noted as asked for just now.
    fn asked_again(&mut self, key: &PictureKey) -> Option<Entity<Picture>> {
        let held = self.held.get(key)?.clone();
        self.asked.retain(|asked| asked != key);
        self.asked.push_back(key.clone());
        Some(held)
    }

    /// Holds `picture` under `key` to be loaded, and lets go of the picture of its size asked for
    /// longest ago where more than the size's most are held.
    fn hold(&mut self, key: PictureKey, picture: Entity<Picture>) {
        let size = key.size;
        self.held.insert(key.clone(), picture.clone());
        self.asked.push_back(key.clone());
        self.waiting.push((key, picture));
        let held_of_size = self.asked.iter().filter(|asked| asked.size == size).count();
        if held_of_size > size.most_held() {
            let oldest = self.asked.iter().position(|asked| asked.size == size);
            if let Some(let_go) = oldest.and_then(|index| self.asked.remove(index)) {
                self.held.remove(&let_go);
            }
        }
    }

    /// The next picture to load, where fewer than [`MOST_AT_ONCE`] are loading.
    fn next_to_load(&mut self) -> Option<(PictureKey, Entity<Picture>)> {
        if self.loading >= MOST_AT_ONCE {
            return None;
        }
        let next = self.waiting.pop()?;
        self.loading += 1;
        Some(next)
    }
}

/// The picture at `address`, decoded for `size`: as far as it has come, loading from now where it
/// was not asked for before.
pub(crate) fn picture(
    app: &mut App,
    reader: &ReaderApp,
    address: &str,
    size: PictureSize,
) -> Entity<Picture> {
    let key = PictureKey {
        address: address.to_owned(),
        size,
    };
    let held = reader
        .pictures
        .update(app, |pictures, _cx| pictures.asked_again(&key));
    if let Some(held) = held {
        return held;
    }
    let picture = app.new_entity(|_cx| Picture::Loading);
    reader
        .pictures
        .update(app, |pictures, _cx| pictures.hold(key, picture.clone()));
    load_waiting(app, reader);
    picture
}

/// Starts loading the pictures waiting, as many as may load at once.
fn load_waiting(app: &mut App, reader: &ReaderApp) {
    let next = |app: &mut App| {
        reader
            .pictures
            .update(app, |pictures, _cx| pictures.next_to_load())
    };
    while let Some((key, picture)) = next(app) {
        let reader = reader.clone();
        app.spawn(async move |cx| {
            let image = load(cx, &key).await;
            cx.update(|app| loaded(app, &reader, &picture, image));
        });
    }
}

/// `picture` has loaded as `image`, or failed to: it shows so, and the next waiting loads.
fn loaded(app: &mut App, reader: &ReaderApp, picture: &Entity<Picture>, image: Option<Image>) {
    picture.update(app, |picture, cx| {
        *picture = image.map_or(Picture::Missing, Picture::Ready);
        cx.notify();
    });
    reader
        .pictures
        .update(app, |pictures, _cx| pictures.loading -= 1);
    load_waiting(app, reader);
}

/// The picture `key` names: its bytes from where they are kept, or else fetched and kept, then
/// decoded for the key's size.
async fn load(cx: &AsyncApp, key: &PictureKey) -> Option<Image> {
    let encoded = match picture_cache::kept(&key.address) {
        Some(kept) => kept,
        None => {
            let fetched = fetch(&key.address).await?;
            picture_cache::keep(&key.address, &fetched);
            fetched
        }
    };
    decode(cx, encoded, key.size).await
}

async fn fetch(address: &str) -> Option<Vec<u8>> {
    let url = Url::parse(address).ok()?;
    let answer = http::get(&url, ACCEPTED, &Validators::default(), LARGEST_PICTURE).await;
    match answer {
        Ok(Answer::Document(document)) => Some(document.body),
        Ok(Answer::Unchanged) => None,
        Err(error) => {
            eprintln!("the picture at {address} is not fetched: {}", error.said());
            None
        }
    }
}

/// `encoded` decoded by the system's codecs, scaled down to fit `size`, as an image to draw; none
/// where the system has no codecs or does not read it.
async fn decode(cx: &AsyncApp, encoded: Vec<u8>, size: PictureSize) -> Option<Image> {
    let opening = cx.update(|app| {
        let platform = app.platform();
        Some(platform.image_codecs()?.open(Arc::from(encoded)))
    })?;
    let source = opening.await.ok()?;
    let side = size.fits_within();
    let decoding = source.decode_pixels(0, Some(Size::new(side, side)));
    let pixels = decoding.await.ok()?;
    cx.update(|app| app.platform().import_pixels(pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use waft::AppCell;

    fn key(name: &str) -> PictureKey {
        PictureKey {
            address: format!("https://example.org/{name}.jpg"),
            size: PictureSize::Full,
        }
    }

    #[test]
    fn the_picture_asked_for_longest_ago_is_let_go_of_first() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        let mut pictures = Pictures::default();
        let most = PictureSize::Full.most_held();
        for index in 0..most {
            pictures.hold(
                key(&index.to_string()),
                app.new_entity(|_cx| Picture::Loading),
            );
        }
        assert!(pictures.asked_again(&key("0")).is_some());

        pictures.hold(key("one more"), app.new_entity(|_cx| Picture::Loading));
        assert_eq!(pictures.held.len(), most);
        assert!(pictures.held.contains_key(&key("0")), "asked for again");
        assert!(!pictures.held.contains_key(&key("1")));
    }

    #[test]
    fn no_more_pictures_load_at_once_than_may_and_the_last_asked_for_goes_first() {
        let cell = AppCell::new();
        let mut app = cell.borrow_mut();
        let mut pictures = Pictures::default();
        for index in 0..=MOST_AT_ONCE {
            pictures.hold(
                key(&index.to_string()),
                app.new_entity(|_cx| Picture::Loading),
            );
        }
        let first = pictures.next_to_load().unwrap().0;
        assert_eq!(first, key(&MOST_AT_ONCE.to_string()));
        for _ in 1..MOST_AT_ONCE {
            assert!(pictures.next_to_load().is_some());
        }
        assert!(pictures.next_to_load().is_none(), "one waits");
    }
}
