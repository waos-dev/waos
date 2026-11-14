//! The kind of file a note is imported from and exported to: plain text, the note's text as it
//! is, named after its title.

use std::path::Path;

use waft_embedder::FileType;

/// Plain text, as the file dialogs offer it.
pub(crate) fn plain_text() -> FileType {
    FileType {
        label: "Plain text".to_owned(),
        extensions: vec!["txt".to_owned()],
        mime_types: vec!["text/plain".to_owned()],
    }
}

/// The name the save dialog suggests for a note titled `title`: the title, without the characters
/// a file's name cannot hold on the desktop systems, as a text file.
pub(crate) fn file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character => character,
        })
        .collect();
    format!("{}.txt", name.trim_matches(['.', ' ']))
}

/// The name of the file at `path`, as the window says it.
pub(crate) fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notes_file_is_named_after_its_title() {
        assert_eq!(file_name("Shopping"), "Shopping.txt");
        assert_eq!(file_name("To do: 1/2"), "To do- 1-2.txt");
        assert_eq!(file_name("...hidden "), "hidden.txt");
    }
}
