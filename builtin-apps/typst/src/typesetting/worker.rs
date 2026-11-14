//! The typesetting worker: Typst's compiler, renderer and PDF exporter, run in an instance of the
//! program of their own, so the window goes on answering while a document is set, a page drawn or
//! a PDF written. It answers the window's requests one at a time, in the order asked, over the
//! pages it set last. Its fonts are the program's and the system's, the book of them made as it
//! starts.
//!
//! Pages with an error in their text are not set: the pages of the last text without one stay,
//! drawn and exported as long as they do, and a PDF of the text as it is is refused.

use typst::utils::{Scalar, hash128};
use typst_layout::{Page, PagedDocument};
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;
use wasdk::{StreamReader, StreamWriter};

use super::messages::{
    DrawnPage, PageOutline, PagePicture, PdfRefusal, Reply, Request, SetPages, decoded, encoded,
};
use super::{DocumentFonts, Typesetter};

/// Answers each request the window sends, in order, until the window stops sending.
#[wasdk::worker]
pub(crate) async fn typesetter(
    mut requests: StreamReader<Vec<u8>>,
    mut replies: StreamWriter<Vec<u8>>,
) {
    let mut typesetting = Typesetting::new(DocumentFonts::with_system());
    while let Some(request) = requests.next().await {
        let Some(request) = decoded(&request) else {
            continue;
        };
        let reply = typesetting.answer(request);
        if replies.write_one(encoded(&reply)).await.is_some() {
            return;
        }
    }
}

/// Typst over the document, and the pages it set last.
pub(crate) struct Typesetting {
    typesetter: Typesetter,
    set: Option<PagedDocument>,
    /// Whether the pages are of the text as last set, which they are not while it has an error.
    current: bool,
}

impl Typesetting {
    /// Typst over an untitled, empty document, set in `fonts`.
    pub(crate) fn new(fonts: DocumentFonts) -> Typesetting {
        Typesetting {
            typesetter: Typesetter::new(fonts),
            set: None,
            current: false,
        }
    }

    pub(crate) fn answer(&mut self, request: Request) -> Reply {
        match request {
            Request::Typeset { path, text } => {
                self.typesetter.set_path(path.as_deref());
                Reply::Set(self.typeset(&text))
            }
            Request::Draw {
                index,
                mark,
                density,
            } => Reply::Drawn(DrawnPage {
                index,
                mark,
                density,
                picture: self.draw(index, mark, density),
            }),
            Request::ExportPdf => Reply::Pdf(self.pdf()),
        }
    }

    /// Sets `text` as pages, kept where it has no error.
    fn typeset(&mut self, text: &str) -> SetPages {
        let typeset = self.typesetter.typeset(text);
        self.current = typeset.pages.is_some();
        let outlines = typeset
            .pages
            .as_ref()
            .map(|pages| pages.pages().iter().map(outline_of).collect());
        if let Some(pages) = typeset.pages {
            self.set = Some(pages);
        }
        SetPages {
            pages: outlines,
            problems: typeset.problems,
            closed_folder: typeset.closed_folder,
            took: typeset.took,
        }
    }

    /// The page at `index` drawn at `density`, where the pages set last have it with `mark`.
    fn draw(&self, index: usize, mark: u128, density: u32) -> Option<PagePicture> {
        let page = self.set.as_ref()?.pages().get(index)?;
        (mark_of(page) == mark).then(|| picture_of(page, density))
    }

    /// The pages as a PDF, where they are of the text as it is.
    fn pdf(&self) -> Result<Vec<u8>, PdfRefusal> {
        let document = self
            .set
            .as_ref()
            .filter(|_| self.current)
            .ok_or(PdfRefusal::HasErrors)?;
        typst_pdf::pdf(document, &PdfOptions::default()).map_err(|errors| {
            let messages: Vec<&str> = errors.iter().map(|error| error.message.as_str()).collect();
            PdfRefusal::Failed(messages.join("; "))
        })
    }
}

fn outline_of(page: &Page) -> PageOutline {
    let size = page.frame.size();
    PageOutline {
        size: [size.x.to_pt(), size.y.to_pt()],
        mark: mark_of(page),
    }
}

/// What `page` shows, as a hash: a page whose mark is unchanged need not be drawn again.
fn mark_of(page: &Page) -> u128 {
    hash128(&(&page.frame, &page.fill))
}

/// `page` drawn by Typst's renderer at `density` pixels to a thousand points.
fn picture_of(page: &Page, density: u32) -> PagePicture {
    let options = RenderOptions {
        pixel_per_pt: Scalar::new(f64::from(density) / 1000.0),
        render_bleed: false,
    };
    let pixmap = typst_render::render(page, &options);
    PagePicture {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: pixmap.take(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::welcome::WELCOME;

    fn typeset(typesetting: &mut Typesetting, text: &str) -> SetPages {
        let Reply::Set(set) = typesetting.answer(Request::Typeset {
            path: None,
            text: text.to_owned(),
        }) else {
            panic!("a typeset answers the pages set");
        };
        set
    }

    #[test]
    fn the_starting_document_is_set_drawn_and_exported_as_a_pdf() {
        let mut typesetting = Typesetting::new(DocumentFonts::carried());
        let set = typeset(&mut typesetting, WELCOME);
        let first = set.pages.expect("pages")[0];
        let Reply::Drawn(drawn) = typesetting.answer(Request::Draw {
            index: 0,
            mark: first.mark,
            density: 1000,
        }) else {
            panic!("a draw answers the page drawn");
        };
        let picture = drawn.picture.expect("the page as set is drawn");
        assert_eq!(
            picture.rgba.len(),
            (picture.width * picture.height * 4) as usize
        );
        let Reply::Pdf(Ok(pdf)) = typesetting.answer(Request::ExportPdf) else {
            panic!("the pages are exported");
        };
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_page_set_again_since_is_not_drawn() {
        let mut typesetting = Typesetting::new(DocumentFonts::carried());
        let first = typeset(&mut typesetting, "One page.").pages.unwrap()[0];
        typeset(&mut typesetting, "Another page.");
        let Reply::Drawn(drawn) = typesetting.answer(Request::Draw {
            index: 0,
            mark: first.mark,
            density: 1000,
        }) else {
            panic!("a draw answers");
        };
        assert!(drawn.picture.is_none());
    }

    #[test]
    fn a_text_with_an_error_keeps_the_last_pages_and_is_not_exported() {
        let mut typesetting = Typesetting::new(DocumentFonts::carried());
        typeset(&mut typesetting, "One page.");
        let set = typeset(&mut typesetting, "The #unknown is here.");
        assert!(set.pages.is_none());
        assert!(
            typesetting.set.is_some(),
            "the last pages without an error stay"
        );
        assert!(matches!(
            typesetting.answer(Request::ExportPdf),
            Reply::Pdf(Err(PdfRefusal::HasErrors))
        ));
    }

    #[test]
    fn a_reply_crosses_as_bytes_and_back() {
        let mut typesetting = Typesetting::new(DocumentFonts::carried());
        let reply = Reply::Set(typeset(&mut typesetting, WELCOME));
        let Some(Reply::Set(crossed)) = decoded::<Reply>(&encoded(&reply)) else {
            panic!("the reply decodes as it was");
        };
        let Reply::Set(sent) = reply else {
            unreachable!()
        };
        assert_eq!(crossed.pages, sent.pages);
        assert_eq!(crossed.problems, sent.problems);
    }
}
