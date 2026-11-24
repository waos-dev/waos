//! Where on the phone a pick the system gives without a path is. Android answers a pick as a
//! document's `content:` URI; one from the phone's own storage (`com.android.externalstorage
//! .documents`) names its volume and its path in it, `primary:Music/a.mp3`, and a download, its
//! path outright, `raw:/storage/…`. WaOS mounts those paths for the program as any pick.
//!
//! The invariant: this only says where a pick is, never whether WaOS may read it there, which is
//! the system's to say (its All files access, asked through the shell's system permissions).

use std::path::{Path, PathBuf};

/// The provider of the phone's own storage, whose document ids are a volume and a path in it.
const EXTERNAL_STORAGE: &str = "com.android.externalstorage.documents";
/// The provider of downloads, some of whose document ids are their path, `raw:/storage/…`.
const DOWNLOADS: &str = "com.android.providers.downloads.documents";
/// The volume Android names the phone's shared storage by in a document id.
const PRIMARY: &str = "primary";
/// Where Android mounts every volume of the phone's storage, a removable one by its id:
/// `/storage/1234-ABCD`.
const VOLUMES: &str = "/storage";

/// The path on the phone of the pick Android names by `url`, the phone's shared storage being at
/// `shared`, which Android answers as `Environment.getExternalStorageDirectory`; or why WaOS
/// cannot mount it, where it is not on the phone's own storage.
pub fn path_on_phone_storage(url: &str, shared: &Path) -> Result<PathBuf, String> {
    let (authority, document) = content_document(url).ok_or_else(not_on_the_phone)?;
    match authority {
        EXTERNAL_STORAGE => storage_path(shared, &document),
        DOWNLOADS => document
            .strip_prefix("raw:")
            .map(PathBuf::from)
            .ok_or_else(not_on_the_phone),
        _ => Err(not_on_the_phone()),
    }
}

/// Whether `path` is on the phone's own storage, shared or removable, which Android lets an app
/// reach by its paths only with All files access; the shared storage is at `shared`.
pub fn is_on_phone_storage(path: &Path, shared: &Path) -> bool {
    path.starts_with(shared) || path.starts_with(VOLUMES)
}

/// The path of `document`, a volume and a path in it, as the device's storage provider names it:
/// `primary:Music/a.mp3` in the shared storage at `shared`, `1234-ABCD:Music` on that volume.
fn storage_path(shared: &Path, document: &str) -> Result<PathBuf, String> {
    let (volume, path) = document.split_once(':').ok_or_else(not_on_the_phone)?;
    let root = if volume == PRIMARY {
        shared.to_path_buf()
    } else {
        Path::new(VOLUMES).join(volume)
    };
    Ok(if path.is_empty() {
        root
    } else {
        root.join(path)
    })
}

fn not_on_the_phone() -> String {
    "WaOS mounts only what is on the phone's own storage, and this pick is not".to_owned()
}

/// The authority of a `content:` URI and its last document id, decoded: the document picked, or,
/// in a tree's URI, the folder in it.
fn content_document(url: &str) -> Option<(&str, String)> {
    let rest = url.strip_prefix("content://")?;
    let (authority, path) = rest.split_once('/')?;
    let encoded = path.rsplit_once("document/").map(|(_, id)| id)?;
    Some((authority, percent_decoded(encoded)?))
}

/// `encoded` with its `%XX` escapes decoded, or none where they make no UTF-8.
fn percent_decoded(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = encoded.get(at + 1..at + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED: &str = "/storage/emulated/0";

    fn path_of(url: &str) -> Result<PathBuf, String> {
        path_on_phone_storage(url, Path::new(SHARED))
    }

    #[test]
    fn a_pick_on_the_shared_storage_is_its_path() {
        let file = "content://com.android.externalstorage.documents/document/primary%3AMusic%2Fhatenaki.mp3";
        assert_eq!(
            path_of(file),
            Ok(PathBuf::from("/storage/emulated/0/Music/hatenaki.mp3"))
        );
        let folder = "content://com.android.externalstorage.documents/tree/primary%3AMusic/document/primary%3AMusic";
        assert_eq!(
            path_of(folder),
            Ok(PathBuf::from("/storage/emulated/0/Music"))
        );
        let root =
            "content://com.android.externalstorage.documents/tree/primary%3A/document/primary%3A";
        assert_eq!(path_of(root), Ok(PathBuf::from("/storage/emulated/0")));
    }

    #[test]
    fn a_pick_on_a_removable_volume_is_under_storage() {
        let folder = "content://com.android.externalstorage.documents/tree/1234-ABCD%3AMusic/document/1234-ABCD%3AMusic%2FLive";
        assert_eq!(
            path_of(folder),
            Ok(PathBuf::from("/storage/1234-ABCD/Music/Live"))
        );
    }

    #[test]
    fn a_download_named_by_its_path_is_that_path() {
        let file = "content://com.android.providers.downloads.documents/document/raw%3A%2Fstorage%2Femulated%2F0%2FDownload%2Fa.mp3";
        assert_eq!(
            path_of(file),
            Ok(PathBuf::from("/storage/emulated/0/Download/a.mp3"))
        );
    }

    #[test]
    fn a_pick_off_the_phone_s_own_storage_says_why() {
        let drive = "content://com.google.android.apps.docs.storage/document/acc%3D1%3Bdoc%3D2";
        let elsewhere = path_of(drive).unwrap_err();
        assert!(elsewhere.contains("phone's own storage"), "{elsewhere}");
        let download = "content://com.android.providers.downloads.documents/document/msf%3A42";
        assert!(path_of(download).is_err());
        assert!(path_of("blob:https://example.com/1").is_err());
    }

    #[test]
    fn the_phone_s_own_storage_is_its_shared_storage_and_its_removable_volumes() {
        let shared = Path::new(SHARED);
        assert!(is_on_phone_storage(
            Path::new("/storage/emulated/0/Music"),
            shared
        ));
        assert!(is_on_phone_storage(
            Path::new("/storage/1234-ABCD/Music"),
            shared
        ));
        assert!(!is_on_phone_storage(
            Path::new("/data/user/0/dev.waos.app/files/notes"),
            shared
        ));
    }
}
