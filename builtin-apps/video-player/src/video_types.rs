//! The videos the app opens and lists: what Apple's players play, by their files' extensions and
//! MIME types.

use std::path::Path;

/// The extensions of the files the app opens and lists, in lower case.
pub(crate) const EXTENSIONS: [&str; 6] = ["mp4", "m4v", "mov", "qt", "3gp", "3g2"];

/// The MIME types of those files.
pub(crate) const MIME_TYPES: [&str; 5] = [
    "video/mp4",
    "video/x-m4v",
    "video/quicktime",
    "video/3gpp",
    "video/3gpp2",
];

/// Whether `path` names a video the app plays, by its extension, in any case.
pub(crate) fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_is_known_by_its_extension_in_any_case() {
        assert!(is_video(Path::new("/films/Holiday.MOV")));
        assert!(is_video(Path::new("/films/a.b.mp4")));
        assert!(!is_video(Path::new("/films/notes.txt")));
        assert!(!is_video(Path::new("/films/mp4")));
    }
}
