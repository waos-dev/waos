//! A program that reads the fonts it was given, as a text stack would: it counts the installed
//! families, finds the face covering a letter, its family's faces and the bold among them, finds
//! the system font, reads its character map from its `cmap` table to find the letter's glyph,
//! opens the system font's whole file, reads it into a buffer of the length the engine gives, and
//! finds the same glyph in it, as a program that reads fonts itself would, and asks the engine for
//! that glyph's coverage and outline. A program given no fonts hears that there are none: no
//! family, no face, and no glyph from a font it names anyway.
//!
//! ```text
//! cargo build -p fonts --target wasm32-wasip3
//! ```

use read_fonts::tables::cmap::Cmap;
use read_fonts::{FontData, FontRead, FontRef, TableProvider};
use wasdk::StreamResult;
use wasdk::fonts::{self, Font, FontFile, PathElement, Slant, Style, Typeface};

/// The letter whose glyph is drawn.
const LETTER: char = 'a';

/// The size the system font is asked for, in points, and its glyph drawn at, in pixels.
const POINTS: f32 = 13.0;
const PIXELS: f32 = 32.0;

/// The character map's tag, its four bytes as a big-endian number.
const CMAP: u32 = u32::from_be_bytes(*b"cmap");

const REGULAR: Style = Style {
    weight: 400,
    width: 100.0,
    slant: Slant::Upright,
};

#[wasdk::main]
async fn main() -> Result<(), String> {
    println!("families: {}", fonts::families().len());
    let covering = fonts::match_character(None, REGULAR, &["en".to_owned()], LETTER);
    println!("covering {LETTER:?}: {}", described(covering.as_ref()));
    if let Some(covering) = covering {
        describe_family(&covering.family());
    }
    let Some(system) = fonts::system_font(POINTS, REGULAR) else {
        println!("system font: none");
        draw(&font_named(1), 1);
        return Ok(());
    };
    println!("system font: {}", described(Some(&system)));
    let glyph = glyph_of(&system, LETTER)?;
    println!("glyph of {LETTER:?}: {glyph}");
    describe_file(&system).await?;
    draw(&font_named(system.id()), glyph);
    Ok(())
}

/// How many faces `family` has, and which is its bold.
fn describe_family(family: &str) {
    println!("{family}: {} faces", fonts::family(family).len());
    let bold = Style {
        weight: 700,
        ..REGULAR
    };
    let nearest = fonts::match_family_style(family, bold);
    println!("its bold: {}", described(nearest.as_ref()));
}

/// A typeface as the program says it: its family and style, or none.
fn described(typeface: Option<&Typeface>) -> String {
    let Some(typeface) = typeface else {
        return "none".to_owned();
    };
    let style = typeface.style();
    format!(
        "{}, weight {}, {:?}",
        typeface.family(),
        style.weight,
        style.slant
    )
}

/// The glyph `typeface` maps `character` to, read from its character map.
fn glyph_of(typeface: &Typeface, character: char) -> Result<u32, String> {
    let bytes = typeface
        .table(CMAP)
        .ok_or("the system font has no character map")?;
    println!("cmap: {} bytes", bytes.len());
    let map = Cmap::read(FontData::new(&bytes)).map_err(|error| format!("cmap: {error}"))?;
    let glyph = map
        .map_codepoint(character)
        .ok_or_else(|| format!("the system font maps no glyph to {character:?}"))?;
    Ok(glyph.to_u32())
}

/// How large `typeface`'s file is, which face of it the typeface is, and the glyph its own
/// character map gives the letter, read from the file whole.
async fn describe_file(typeface: &Typeface) -> Result<(), String> {
    let file = typeface.open_stream();
    let index = file.index;
    let bytes = read_whole(file).await?;
    let face = FontRef::from_index(&bytes, index).map_err(|error| format!("the file: {error}"))?;
    let glyph = face
        .cmap()
        .ok()
        .and_then(|map| map.map_codepoint(LETTER))
        .map_or(0, |glyph| glyph.to_u32());
    println!(
        "file: {} bytes, face {index}, glyph of {LETTER:?} {glyph}",
        bytes.len()
    );
    Ok(())
}

/// `file`'s bytes, read into a buffer of the length it says, reserved once; a stream that ends
/// short of it or runs past it is an error.
async fn read_whole(file: FontFile) -> Result<Vec<u8>, String> {
    let FontFile {
        mut data, length, ..
    } = file;
    let length = usize::try_from(length).map_err(|_| format!("a file of {length} bytes"))?;
    let mut bytes = Vec::with_capacity(length);
    while bytes.len() < length {
        let (result, read) = data.read(bytes).await;
        bytes = read;
        if result == StreamResult::Dropped {
            return Err(format!(
                "the file ended at {} of {length} bytes",
                bytes.len()
            ));
        }
    }
    if data.next().await.is_some() {
        return Err(format!("the file ran past its {length} bytes"));
    }
    Ok(bytes)
}

/// `typeface` at its default: no point on its axes, and no fake bold or italic.
fn font_named(typeface: u64) -> Font {
    Font {
        typeface,
        variation_coordinates: Vec::new(),
        embolden: 0.0,
        skew: 0.0,
    }
}

/// Asks for `glyph`'s coverage and outline, and says what came back.
fn draw(font: &Font, glyph: u32) {
    match fonts::glyph_alpha(font, glyph, PIXELS, 0.0) {
        Some(image) => {
            let inked = image.data.iter().filter(|&&coverage| coverage > 0).count();
            println!(
                "coverage: {}x{}, {inked} pixels inked",
                image.width, image.height
            );
        }
        None => println!("coverage: none"),
    }
    match fonts::glyph_path(font, glyph, PIXELS) {
        Some(outline) => {
            let curves = outline
                .iter()
                .filter(|element| !matches!(element, PathElement::MoveTo(_) | PathElement::Close))
                .count();
            println!("outline: {} elements, {curves} drawing", outline.len());
        }
        None => println!("outline: none"),
    }
}
