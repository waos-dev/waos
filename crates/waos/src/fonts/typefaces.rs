//! The faces handed out to one program, each under an id of its own, and the files they are in,
//! each under an id of its own: the program names a face to a glyph call by its id, and the engine
//! finds the face's file and index by it; the program knows a file it has read already by the
//! file's id.
//!
//! The invariant: an id names one face for as long as the program runs, and a face handed out
//! again, however it was matched, keeps its id; a file's id is the same for every face in it and
//! different for different files. A face is known by the address of its file's bytes and its
//! index in the file, and a file by the address of its bytes; holding the bytes is what makes the
//! address a sound key, since a file kept here is never freed while the program runs, so no other
//! file can land on its address and take over its ids.
//!
//! A file is its bytes as the manager shares them. valo's managers map each file once, by its
//! path, and share the one mapping among its faces: CoreText on macOS by the path CoreText gives,
//! the Android manager by the path in the system's font configuration, and the scan on Windows
//! and Linux by the path fontdb found the file at. A manager that read one file twice would give
//! it two ids, and a program would read it twice; two files never share one.

use std::collections::HashMap;
use std::sync::Arc;

use valo_fontmgr::{FontData, Typeface};

/// What a face is handed out with: its id and the id of the file it is in.
pub(super) struct Ids {
    pub(super) face: u64,
    pub(super) file: u64,
}

/// The faces handed out, each with the file it is in, as valo shares its bytes, and its index
/// there.
#[derive(Default)]
pub(super) struct Typefaces {
    /// Each face's file and index, at its id less one: ids count from 1, so 0 names nothing.
    faces: Vec<(FontData, u32)>,
    /// Each face's id, by the address of its file's bytes and its index.
    face_ids: HashMap<(usize, u32), u64>,
    /// Each file's id, by the address of its bytes; ids count from 1.
    file_ids: HashMap<usize, u64>,
}

impl Typefaces {
    /// `face`'s ids, given now if the face or its file is new to the program.
    pub(super) fn ids(&mut self, face: &Typeface) -> Ids {
        let data = face.shared_data();
        Ids {
            file: self.file_id(&data),
            face: self.face_id(data, face.index()),
        }
    }

    /// The file and index of the face handed out as `id`; none for an id the program was never
    /// given.
    pub(super) fn face(&self, id: u64) -> Option<&(FontData, u32)> {
        let index = usize::try_from(id.checked_sub(1)?).ok()?;
        self.faces.get(index)
    }

    /// The id of the face at `index` in `data`, keeping the face if it is new.
    fn face_id(&mut self, data: FontData, index: u32) -> u64 {
        let key = (address(&data), index);
        if let Some(&id) = self.face_ids.get(&key) {
            return id;
        }
        self.faces.push((data, index));
        let id = self.faces.len() as u64;
        self.face_ids.insert(key, id);
        id
    }

    /// The id of the file whose bytes are `data`; a face of it is kept by [`face_id`], which
    /// keeps the address the id is known by.
    ///
    /// [`face_id`]: Typefaces::face_id
    fn file_id(&mut self, data: &FontData) -> u64 {
        let next = self.file_ids.len() as u64 + 1;
        *self.file_ids.entry(address(data)).or_insert(next)
    }
}

/// Where a file's bytes are, which no other file's are while these are kept.
fn address(data: &FontData) -> usize {
    Arc::as_ptr(data).cast::<u8>() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    use valo_fontmgr::Style;

    fn file(bytes: &[u8]) -> FontData {
        Arc::new(bytes.to_vec())
    }

    fn face_of(data: &FontData, index: u32) -> Typeface {
        Typeface::new(Arc::clone(data), index, "First", Style::default())
    }

    #[test]
    fn the_faces_of_one_file_have_ids_of_their_own_and_its_file_id() {
        let collection = file(&[1, 2, 3]);
        let mut typefaces = Typefaces::default();
        let first = typefaces.ids(&face_of(&collection, 0));
        let second = typefaces.ids(&face_of(&collection, 1));
        assert_eq!((first.face, second.face), (1, 2));
        assert_eq!(first.file, second.file);
    }

    #[test]
    fn two_files_have_two_file_ids() {
        let mut typefaces = Typefaces::default();
        let first = typefaces.ids(&face_of(&file(&[1, 2, 3]), 0));
        let second = typefaces.ids(&face_of(&file(&[1, 2, 3]), 0));
        assert_ne!(first.file, second.file);
        assert_ne!(first.face, second.face);
    }

    #[test]
    fn a_face_handed_out_again_keeps_its_ids() {
        let data = file(&[1, 2, 3]);
        let mut typefaces = Typefaces::default();
        let by_family = typefaces.ids(&face_of(&data, 0));
        let by_character = typefaces.ids(&face_of(&data, 0));
        assert_eq!(by_family.face, by_character.face);
        assert_eq!(by_family.file, by_character.file);
    }

    #[test]
    fn an_id_names_its_face_and_no_other_id_names_one() {
        let data = file(&[1, 2, 3]);
        let mut typefaces = Typefaces::default();
        let id = typefaces.ids(&face_of(&data, 2)).face;
        let (found, index) = typefaces.face(id).unwrap();
        assert!(Arc::ptr_eq(found, &data));
        assert_eq!(*index, 2);
        for never_given in [0, id + 1, u64::MAX] {
            assert!(typefaces.face(never_given).is_none(), "{never_given}");
        }
    }
}
