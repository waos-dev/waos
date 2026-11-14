//! The pages' pictures: each page drawn by Typst's renderer as pixels at the screen's density,
//! and given to Waft as an image to show.
//!
//! Pages are drawn as pixels, not as Waft's own vector drawing, because Typst's renderer is the
//! one its own tools export PNGs with: every feature of a page (gradients, patterns, clipping,
//! colour glyphs, embedded pictures) looks as it will in the PDF, with no second renderer to
//! keep in step.
//!
//! The typesetting worker draws them, so the window goes on answering while it does: one page at
//! a time, only those the preview has built, in the order it asked, the next asked for once the
//! last has arrived, so a page the user scrolls to waits for one page at most. A picture is kept
//! while its page's mark and density still hold, so typing on one page draws only that page
//! again; a picture that no longer holds stays shown until its page is drawn anew, and one that
//! arrives for a page set again since is let go.

use std::collections::HashMap;
use std::time::Duration;

use waft::ui::Image;
use waft::ui::image_codecs::{AlphaType, PixelBuffer, PixelFormat, PixelLayout};
use waft::{App, Listener, Timer};

use crate::documents_app::DocumentsApp;
use crate::typesetting::messages::{DrawnPage, PagePicture, Request};

/// The longest side a picture may have, in pixels, which every GPU takes as a texture.
const LONGEST_SIDE: f64 = 4096.0;

/// How many bytes of pictures are kept before those farthest from the page last drawn go.
const BYTES_KEPT: usize = 192 << 20;

/// How long a page already shown at another density waits to be drawn again, so that a window
/// being resized draws it once it rests.
const SETTLE: Duration = Duration::from_millis(120);

#[derive(Default)]
pub(crate) struct PageImages {
    shown: HashMap<usize, PageImage>,
    wanted: Vec<Wanted>,
    drawing: bool,
}

struct PageImage {
    image: Image,
    drawn_as: Drawn,
    bytes: usize,
}

/// What a picture shows: the page with this mark, at this many pixels to a thousand points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drawn {
    mark: u128,
    density: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wanted {
    index: usize,
    drawn_as: Drawn,
}

impl PageImages {
    /// The picture of the page at `index`, as last drawn.
    pub(crate) fn image(&self, index: usize) -> Option<Image> {
        self.shown.get(&index).map(|page| page.image.clone())
    }

    /// Lets go of every picture, as another document's pages are none of this one's.
    pub(crate) fn forget(&mut self) {
        self.shown.clear();
        self.wanted.clear();
    }

    /// The document was set anew, with `count` pages: pictures of pages it no longer has go, and
    /// nothing asked for before is still wanted.
    pub(crate) fn pages_set(&mut self, count: usize) {
        self.shown.retain(|index, _| *index < count);
        self.wanted.clear();
    }

    /// Asks for the page at `index`, with `mark`, drawn at `pixels_per_point`; answers how long
    /// to wait before drawing it, or none where its picture holds or is asked for already.
    fn want(&mut self, index: usize, mark: u128, pixels_per_point: f64) -> Option<Duration> {
        let drawn_as = Drawn {
            mark,
            density: (pixels_per_point * 1000.0).round() as u32,
        };
        let shown = self.shown.get(&index).map(|page| page.drawn_as);
        if shown == Some(drawn_as) {
            self.wanted.retain(|wanted| wanted.index != index);
            return None;
        }
        let wanted = Wanted { index, drawn_as };
        if self.wanted.contains(&wanted) {
            return None;
        }
        self.wanted.retain(|wanted| wanted.index != index);
        self.wanted.push(wanted);
        let only_resized = shown.is_some_and(|shown| shown.mark == mark);
        Some(if only_resized { SETTLE } else { Duration::ZERO })
    }

    /// Keeps `page` as the picture of the page at `index`, and lets go of the pictures farthest
    /// from it while they are more than [`BYTES_KEPT`].
    fn keep(&mut self, index: usize, page: PageImage) {
        self.shown.insert(index, page);
        let mut farthest: Vec<usize> = self.shown.keys().copied().collect();
        farthest.sort_by_key(|other| std::cmp::Reverse(other.abs_diff(index)));
        let mut bytes: usize = self.shown.values().map(|page| page.bytes).sum();
        for other in farthest {
            if bytes <= BYTES_KEPT || other == index {
                break;
            }
            bytes -= self.shown.remove(&other).map_or(0, |page| page.bytes);
        }
    }
}

/// The most pixels to a point a page `size` points wide and high is drawn at.
pub(crate) fn densest(size: [f64; 2]) -> f64 {
    LONGEST_SIDE / size[0].max(size[1]).max(1.0)
}

/// Asks for the picture of the page at `index`, drawn at `pixels_per_point`, where the one shown
/// does not hold; it is drawn after the event being handled.
pub(crate) fn want_page(
    app: &mut App,
    documents: &DocumentsApp,
    index: usize,
    pixels_per_point: f64,
) {
    let Some(mark) = documents.pages.read(app).mark(index) else {
        return;
    };
    let (wait, start) = documents.page_images.update(app, |images, _cx| {
        let wait = images.want(index, mark, pixels_per_point);
        let start = wait.is_some() && !images.drawing;
        images.drawing |= start;
        (wait, start)
    });
    if let (Some(wait), true) = (wait, start) {
        draw_after(app, documents, wait);
    }
}

fn draw_after(app: &mut App, documents: &DocumentsApp, wait: Duration) {
    let documents = documents.clone();
    Timer::new(
        app,
        wait,
        Listener::new(move |app| draw_next(app, &documents)),
    );
}

/// Asks the worker for the page asked for first, where its page is still set as it was asked
/// for; the next is asked for once it has arrived ([`page_drawn`]).
fn draw_next(app: &mut App, documents: &DocumentsApp) {
    loop {
        let next = documents.page_images.update(app, |images, _cx| {
            if images.wanted.is_empty() {
                images.drawing = false;
                return None;
            }
            Some(images.wanted.remove(0))
        });
        let Some(wanted) = next else {
            return;
        };
        if documents.pages.read(app).mark(wanted.index) == Some(wanted.drawn_as.mark) {
            documents.typesetting.ask(Request::Draw {
                index: wanted.index,
                mark: wanted.drawn_as.mark,
                density: wanted.drawn_as.density,
            });
            return;
        }
    }
}

/// The worker drew a page as `drawn` says: its picture is kept where its page is still set as it
/// was drawn, and the next page asked for is asked of the worker.
pub(crate) fn page_drawn(app: &mut App, documents: &DocumentsApp, drawn: DrawnPage) {
    let still_set = documents.pages.read(app).mark(drawn.index) == Some(drawn.mark);
    let drawn_as = Drawn {
        mark: drawn.mark,
        density: drawn.density,
    };
    let page = drawn
        .picture
        .filter(|_| still_set)
        .and_then(|picture| page_image(app, picture, drawn_as));
    if let Some(page) = page {
        documents.page_images.update(app, |images, cx| {
            images.keep(drawn.index, page);
            cx.notify();
        });
    }
    draw_next(app, documents);
}

/// `picture`, drawn as `drawn_as` says, as an image to show; none where the platform takes no
/// pixels.
fn page_image(app: &App, picture: PagePicture, drawn_as: Drawn) -> Option<PageImage> {
    let size = [picture.width, picture.height];
    let layout = PixelLayout::packed(size, PixelFormat::Rgba8, AlphaType::Premultiplied);
    let pixels = PixelBuffer::new(layout, picture.rgba).ok()?;
    let bytes = pixels.bytes().len();
    let image = app.platform().import_pixels(pixels)?;
    Some(PageImage {
        image,
        drawn_as,
        bytes,
    })
}
