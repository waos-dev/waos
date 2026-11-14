//! The faces handed out to one program, each under an id of its own: the program names a face to
//! a glyph call by it, and the engine finds the face's file and index by it.
//!
//! The invariant: an id names one face for as long as the program runs, and a face handed out
//! again, however it was matched, keeps its id. A face is known by the address of its file's
//! bytes and its index in the file; holding the bytes is what makes the address a sound key, since
//! a file kept here is never freed while the program runs, so no other file can land on its
//! address and take over its faces' ids.

use std::collections::HashMap;
use std::sync::Arc;

use valo_fontmgr::{FontData, Typeface};

/// The faces handed out, each with the file it is in, as valo shares its bytes, and its index
/// there.
#[derive(Default)]
pub(super) struct Typefaces {
    /// Each face's file and index, at its id less one: ids count from 1, so 0 names nothing.
    faces: Vec<(FontData, u32)>,
    /// Each face's id, by the address of its file's bytes and its index.
    ids: HashMap<(usize, u32), u64>,
}

impl Typefaces {
    /// `face`'s id, given now if the face is new to the program.
    pub(super) fn id(&mut self, face: &Typeface) -> u64 {
        let data = face.shared_data();
        let key = (address(&data), face.index());
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        self.faces.push((data, face.index()));
        let id = self.faces.len() as u64;
        self.ids.insert(key, id);
        id
    }

    /// The file and index of the face handed out as `id`; none for an id the program was never
    /// given.
    pub(super) fn face(&self, id: u64) -> Option<&(FontData, u32)> {
        let index = usize::try_from(id.checked_sub(1)?).ok()?;
        self.faces.get(index)
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

    #[test]
    fn the_faces_of_one_file_have_ids_of_their_own() {
        let collection = file(&[1, 2, 3]);
        let mut typefaces = Typefaces::default();
        let first = Typeface::new(Arc::clone(&collection), 0, "First", Style::default());
        let second = Typeface::new(collection, 1, "First", Style::default());
        assert_eq!((typefaces.id(&first), typefaces.id(&second)), (1, 2));
    }

    #[test]
    fn a_face_handed_out_again_keeps_its_id() {
        let data = file(&[1, 2, 3]);
        let mut typefaces = Typefaces::default();
        let by_family = Typeface::new(Arc::clone(&data), 0, "First", Style::default());
        let by_character = Typeface::new(data, 0, "First", Style::default());
        assert_eq!(typefaces.id(&by_family), typefaces.id(&by_character));
    }

    #[test]
    fn an_id_names_its_face_and_no_other_id_names_one() {
        let data = file(&[1, 2, 3]);
        let mut typefaces = Typefaces::default();
        let id = typefaces.id(&Typeface::new(
            Arc::clone(&data),
            2,
            "First",
            Style::default(),
        ));
        let (found, index) = typefaces.face(id).unwrap();
        assert!(Arc::ptr_eq(found, &data));
        assert_eq!(*index, 2);
        for never_given in [0, id + 1, u64::MAX] {
            assert!(typefaces.face(never_given).is_none(), "{never_given}");
        }
    }
}
