//! GPUI's text over cosmic-text, with the system's fonts read as they are named.
//!
//! cosmic-text shapes and swash rasterizes inside the program, as `gpui_web` does, so it needs
//! a face's whole file. Fonts the program adds (Zed's own) are used as they are. A family the
//! program names that it did not add is the system's: its faces are found with `fonts.family`
//! and each file read whole (`open-stream`). GPUI's `.SystemUIFont` is the system's interface
//! font (`fonts.system-font`). A character no face read so far has is asked of the system:
//! `fonts.match-character` names the face that has it, with the user's languages, which is read
//! and added, and the line is laid out again, cosmic-text's fallback finding it there.
//!
//! A face does not say which file it is in, so each face is read on its own, and the faces of a
//! collection each read its whole file: `Menlo.ttc`'s four faces read its 2.1 MB four times.
//!
//! What it costs: each face read is copied whole into the program's memory (78 MB for PingFang
//! SC, the face macOS has for Chinese, read in 145 ms). WaOS can draw a face's glyphs itself
//! (`fonts.glyph-alpha`) from tables the program never holds, but `gpui_wgpu`'s text system
//! rasterizes with swash from the file and has no seam for another rasterizer.
//!
//! The invariant: a face is read once, a family asked of the system once, and a character no face
//! of the system has asked once.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use anyhow::Result;
use gpui::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, LineLayout,
    MissingGlyphSink, Pixels, PlatformTextSystem, RenderGlyphParams, Size, TextRenderingMode,
};
use gpui_wgpu::CosmicTextSystem;
use wasdk::StreamResult;
use wasdk::fonts::{self, FontFile, Slant, Style, Typeface};
use wasdk::system;

/// The size GPUI's `.SystemUIFont` asks the system's interface font at: any, as one face serves.
const SYSTEM_FONT_SIZE: f32 = 13.0;

/// The family GPUI names the system's interface font by.
const SYSTEM_UI_FONT: &str = ".SystemUIFont";

const REGULAR: Style = Style {
    weight: 400,
    width: 100.0,
    slant: Slant::Upright,
};

pub(crate) struct WasdkTextSystem {
    cosmic: CosmicTextSystem,
    /// The system's interface font's family, which `.SystemUIFont` names, once read.
    system_family: OnceLock<String>,
    read: Mutex<Read>,
}

/// What has been asked of the system so far.
#[derive(Default)]
struct Read {
    faces: HashSet<u64>,
    families: HashSet<String>,
    characters: HashSet<char>,
}

impl WasdkTextSystem {
    /// A text system whose `.SystemUIFont` is the system's interface font.
    pub(crate) fn new() -> WasdkTextSystem {
        WasdkTextSystem {
            cosmic: CosmicTextSystem::new_without_system_fonts(""),
            system_family: OnceLock::new(),
            read: Mutex::new(Read::default()),
        }
    }

    /// The family `.SystemUIFont` is, as cosmic-text names it: the system's interface font, read
    /// the first time it is named. The system's own name for it may differ from its file's
    /// (macOS calls its interface font `.AppleSystemUIFont`), and cosmic-text finds a face by its
    /// file's.
    fn system_family(&self) -> &str {
        self.system_family.get_or_init(|| {
            let Some(face) = fonts::system_font(SYSTEM_FONT_SIZE, REGULAR) else {
                return String::new();
            };
            let before = self.cosmic.all_font_names();
            self.add_face(&mut self.lock(), &face);
            let family = self
                .cosmic
                .all_font_names()
                .into_iter()
                .find(|name| !before.contains(name))
                .unwrap_or_else(|| face.family());
            log::info!("`.SystemUIFont` is {}, by its file {family}", face.family());
            family
        })
    }

    /// Reads the system's faces of `family`, once, unless the program added it: cosmic-text
    /// remembers a family it found no face of, so it must have them before it is asked.
    fn ensure_family(&self, family: &str) {
        if family == SYSTEM_UI_FONT {
            return;
        }
        let family = gpui::font_name_with_fallbacks(family, "");
        let mut read = self.lock();
        if read.families.contains(family) {
            return;
        }
        read.families.insert(family.to_owned());
        if self
            .cosmic
            .all_font_names()
            .iter()
            .any(|name| name == family)
        {
            return;
        }
        for face in fonts::family(family) {
            self.add_face(&mut read, &face);
        }
    }

    /// Reads the system's faces for the characters of `text` that `layout` drew with no glyph;
    /// whether any was added.
    fn add_fallback_faces(&self, text: &str, layout: &LineLayout) -> bool {
        let missing: Vec<char> = layout
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter(|glyph| glyph.id.0 == 0)
            .filter_map(|glyph| text.get(glyph.index..)?.chars().next())
            .collect();
        if missing.is_empty() {
            return false;
        }
        let mut read = self.lock();
        let locales: Vec<String> = system::locales()
            .into_iter()
            .map(|locale| locale.language)
            .collect();
        let mut added = false;
        for character in missing {
            if !read.characters.insert(character) {
                continue;
            }
            if let Some(face) = fonts::match_character(None, REGULAR, &locales, character) {
                added |= self.add_face(&mut read, &face);
            }
        }
        added
    }

    /// Reads `face`'s file into cosmic-text, unless it was read before; whether it was added.
    fn add_face(&self, read: &mut Read, face: &Typeface) -> bool {
        if !read.faces.insert(face.id()) {
            return false;
        }
        let file = face.open_stream();
        let started = Instant::now();
        let bytes = read_font_file(file);
        log::info!(
            "read the system's {} ({} bytes) in {:?}",
            face.family(),
            bytes.len(),
            started.elapsed()
        );
        self.cosmic.add_fonts(vec![Cow::Owned(bytes)]).is_ok()
    }

    fn lock(&self) -> MutexGuard<'_, Read> {
        self.read
            .lock()
            .expect("nothing panics while what was read is locked")
    }
}

/// A face's whole file, read now: GPUI asks for fonts and lays text out without awaiting.
///
/// `wasdk::block_on` runs, and waits for, every task spawned since the program last awaited
/// (wit-bindgen keeps spawned tasks in one list, taken up by whichever task runs next), so a read
/// here would never return behind a task that never ends, as `wasdk-wgpu`'s device listeners
/// never do. So the adapter spawns nothing of its own with `wasdk::spawn`, and `run` awaits once
/// after making the device, before GPUI can ask for a font.
fn read_font_file(file: FontFile) -> Vec<u8> {
    let FontFile {
        mut data, length, ..
    } = file;
    let length = length as usize;
    wasdk::block_on(async move {
        let mut bytes = Vec::with_capacity(length);
        while bytes.len() < length {
            let (result, read) = data.read(bytes).await;
            bytes = read;
            if result == StreamResult::Dropped {
                break;
            }
        }
        // The stream's end, which the platform says at the read after the last byte.
        let _end = data.next().await;
        bytes
    })
}

impl PlatformTextSystem for WasdkTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.cosmic.add_fonts(fonts)
    }

    fn set_missing_glyph_sink(&self, sink: Option<Arc<dyn MissingGlyphSink>>) {
        self.cosmic.set_missing_glyph_sink(sink)
    }

    /// The families read so far and the system's, by name.
    fn all_font_names(&self) -> Vec<String> {
        let mut names = self.cosmic.all_font_names();
        names.extend(fonts::families());
        names.sort_unstable();
        names.dedup();
        names
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        if descriptor.family == SYSTEM_UI_FONT {
            let mut system = descriptor.clone();
            system.family = self.system_family().to_owned().into();
            return self.cosmic.font_id(&system);
        }
        self.ensure_family(&descriptor.family);
        self.cosmic.font_id(descriptor)
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.cosmic.font_metrics(font_id)
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        self.cosmic.typographic_bounds(font_id, glyph_id)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.cosmic.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.cosmic.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.cosmic.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.cosmic.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        let layout = self.cosmic.layout_line(text, font_size, runs);
        if self.add_fallback_faces(text, &layout) {
            return self.cosmic.layout_line(text, font_size, runs);
        }
        layout
    }

    fn recommended_rendering_mode(&self, font_id: FontId, font_size: Pixels) -> TextRenderingMode {
        self.cosmic.recommended_rendering_mode(font_id, font_size)
    }
}
