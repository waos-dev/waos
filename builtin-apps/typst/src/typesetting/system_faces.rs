//! The faces installed on the system, as Typst sees them: each face's entry in Typst's book made
//! from the tables the platform serves, without its file, and its file read through
//! `typeface.open-stream`, into a buffer of the length the platform gives, once Typst asks for the
//! face. A file is read once however many of its faces Typst asks for ([`SystemFiles`], by
//! `typeface.file-id`), and each face is taken from the one copy at its `typeface.index`, as
//! Typst's `Font` shares a file's bytes among the faces made from them.
//!
//! Typst describes a face (`FontInfo`) from a font file's bytes. The platform serves a face's
//! tables but its outlines, and Typst's description reads none of those: so the tables it reads
//! are put together as a font file of their own, which holds no glyph, and described as any file
//! is. The invariant: [`DESCRIBED`] holds every table `FontInfo` reads, and those its parser needs
//! to read a file at all.

use std::collections::HashMap;
use std::sync::Mutex;

use typst::foundations::Bytes;
use typst::text::{Font, FontInfo};
use wasdk::StreamResult;
use wasdk::fonts::{self, FontFile, Tag, Typeface};

/// The tables Typst's description of a face reads: what the parser needs (`head`, `hhea`,
/// `maxp`), the names, the weight, width and panose (`OS/2`), monospace (`post`), the characters
/// covered (`cmap`), maths (`MATH`) and the axes of a variable face (`fvar`).
const DESCRIBED: [&[u8; 4]; 9] = [
    b"head", b"hhea", b"maxp", b"name", b"OS/2", b"post", b"cmap", b"MATH", b"fvar",
];

/// One face installed on the system.
pub(crate) struct SystemFace {
    typeface: Typeface,
}

impl SystemFace {
    /// Every face of every family installed, with Typst's description of it; a face whose tables
    /// Typst cannot read is left out, as Typst's tool leaves out a file it cannot.
    pub(crate) fn installed() -> Vec<(SystemFace, FontInfo)> {
        fonts::families()
            .iter()
            .flat_map(|family| fonts::family(family))
            .filter_map(|typeface| {
                let info = described(&typeface)?;
                Some((SystemFace { typeface }, info))
            })
            .collect()
    }

    /// The face's font, from its whole file as `files` has it; none when the file cannot be
    /// read whole, which is said on the standard error.
    pub(crate) fn load(&self, files: &SystemFiles) -> Option<Font> {
        match files.file_of(&self.typeface) {
            Ok(bytes) => Font::new(bytes, self.typeface.index()),
            Err(error) => {
                eprintln!("{} is not read: {error}", self.typeface.family());
                None
            }
        }
    }
}

/// The system's font files read so far, each by its `typeface.file-id`: the faces of a collection
/// share one copy of their file.
#[derive(Default)]
pub(crate) struct SystemFiles {
    read: Mutex<HashMap<u64, Bytes>>,
}

impl SystemFiles {
    /// The file `typeface` is in, read through the platform's stream the first time one of its
    /// faces is asked for. One file is read at a time, so two faces of one file asked for at once
    /// still read it once.
    fn file_of(&self, typeface: &Typeface) -> Result<Bytes, String> {
        let mut read = self
            .read
            .lock()
            .expect("nothing panics while the files read are locked");
        if let Some(bytes) = read.get(&typeface.file_id()) {
            return Ok(bytes.clone());
        }
        let bytes = Bytes::new(wasdk::block_on(read_whole(typeface.open_stream()))?);
        read.insert(typeface.file_id(), bytes.clone());
        Ok(bytes)
    }
}

/// `file`'s bytes, read into a buffer of the length it says, reserved once, so the worker's
/// memory grows by the file's size and no more; a stream that ends short of it or runs past it
/// is an error.
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

/// Typst's description of `typeface`, from its tables alone.
fn described(typeface: &Typeface) -> Option<FontInfo> {
    let present = typeface.tables();
    let tables: Vec<(Tag, Vec<u8>)> = DESCRIBED
        .iter()
        .map(|name| Tag::from_be_bytes(**name))
        .filter(|tag| present.contains(tag))
        .filter_map(|tag| Some((tag, typeface.table(tag)?)))
        .collect();
    FontInfo::new(&font_file_of(&tables), 0)
}

/// `tables` as an OpenType font file of their own: the offset table, a record for each table in
/// ascending order of tags, then the tables, each at a four-byte boundary. Checksums are left at
/// zero, which no parser Typst uses reads.
fn font_file_of(tables: &[(Tag, Vec<u8>)]) -> Vec<u8> {
    let mut tables: Vec<&(Tag, Vec<u8>)> = tables.iter().collect();
    tables.sort_by_key(|(tag, _)| *tag);
    let count = tables.len() as u16;
    let mut file = Vec::new();
    file.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    file.extend_from_slice(&count.to_be_bytes());
    let (search_range, entry_selector, range_shift) = binary_search_fields(count);
    file.extend_from_slice(&search_range.to_be_bytes());
    file.extend_from_slice(&entry_selector.to_be_bytes());
    file.extend_from_slice(&range_shift.to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    for (tag, bytes) in &tables {
        file.extend_from_slice(&tag.to_be_bytes());
        file.extend_from_slice(&0u32.to_be_bytes());
        file.extend_from_slice(&(offset as u32).to_be_bytes());
        file.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        offset += bytes.len().next_multiple_of(4);
    }
    for (_, bytes) in &tables {
        file.extend_from_slice(bytes);
        file.resize(file.len().next_multiple_of(4), 0);
    }
    file
}

/// The offset table's fields for a binary search over `count` records, as OpenType defines them.
fn binary_search_fields(count: u16) -> (u16, u16, u16) {
    let entry_selector = if count == 0 {
        0
    } else {
        15 - count.leading_zeros() as u16
    };
    let search_range = 16u16 << entry_selector;
    (
        search_range,
        entry_selector,
        (count * 16).saturating_sub(search_range),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled_fonts::LIBERTINUS_SERIF;

    /// The tables of the face in `file` that Typst's description reads, as the platform would
    /// serve them.
    fn described_tables(file: &[u8]) -> Vec<(Tag, Vec<u8>)> {
        let face = ttf_parser::RawFace::parse(file, 0).unwrap();
        DESCRIBED
            .iter()
            .filter_map(|name| {
                let tag = ttf_parser::Tag::from_bytes(name);
                Some((tag.0, face.table(tag)?.to_vec()))
            })
            .collect()
    }

    #[test]
    fn a_face_is_described_from_its_tables_as_from_its_file() {
        let file = LIBERTINUS_SERIF[0];
        let from_tables = FontInfo::new(&font_file_of(&described_tables(file)), 0);
        assert_eq!(from_tables, FontInfo::new(file, 0));
    }
}
