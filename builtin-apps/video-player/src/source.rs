//! What a player opens: a link the user typed or a file they picked, by the URL Waft's `Player`
//! opens, and the name it goes by in the window's title and the recent list.

use url::Url;
use waft_embedder::PickedFile;

/// Media to open, by its URL: an `http` or `https` link, or the `file:` URL of a path inside the
/// program, as a picked file's `PickedFile::url` is. The recent list knows media by it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    url: Url,
}

impl Source {
    /// The link `text` names, spaces around it aside; none unless it is an `http` or `https`
    /// URL.
    pub(crate) fn link(text: &str) -> Option<Source> {
        let url = Url::parse(text.trim()).ok()?;
        is_link(&url).then_some(Source { url })
    }

    /// The file the user picked, where the system gave its path, as WaOS always does.
    pub(crate) fn picked(file: &PickedFile) -> Option<Source> {
        match file {
            PickedFile::Path(_) => Source::kept(&file.url()),
            PickedFile::Handle(_) | PickedFile::Stream(_) => None,
        }
    }

    /// A source by the URL it was kept as: a link's, or a file's.
    pub(crate) fn kept(url: &str) -> Option<Source> {
        let url = Url::parse(url).ok()?;
        (is_link(&url) || url.scheme() == "file").then_some(Source { url })
    }

    pub(crate) fn url(&self) -> &str {
        self.url.as_str()
    }

    /// What it is called: a file's name, or a link's last part, its host where its path has
    /// none.
    pub(crate) fn name(&self) -> String {
        if let Some(name) = self.file_name() {
            return name;
        }
        let last_part = self
            .url
            .path_segments()
            .and_then(|mut parts| parts.rfind(|part| !part.is_empty()));
        last_part
            .or(self.url.host_str())
            .unwrap_or(self.url.as_str())
            .to_owned()
    }

    /// The name of the file a `file:` URL names, its escapes undone.
    fn file_name(&self) -> Option<String> {
        if self.url.scheme() != "file" {
            return None;
        }
        let path = self.url.to_file_path().ok()?;
        Some(path.file_name()?.to_string_lossy().into_owned())
    }
}

fn is_link(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const STREAM: &str = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8";

    #[test]
    fn a_link_is_an_http_or_https_url_and_nothing_else() {
        assert_eq!(
            Source::link(&format!("  {STREAM}\n")).unwrap().url(),
            STREAM
        );
        assert!(Source::link("http://example.com/a.mp4").is_some());
        assert!(Source::link("ftp://example.com/a.mp4").is_none());
        assert!(Source::link("file:///data/a.mp4").is_none());
        assert!(Source::link("a movie").is_none());
    }

    #[test]
    fn a_picked_file_is_its_url_and_goes_by_its_name() {
        let picked = PickedFile::Path(PathBuf::from("/picked/Home Movies/café.mov"));
        let source = Source::picked(&picked).unwrap();
        assert_eq!(source.url(), picked.url());
        assert_eq!(source.name(), "café.mov");
    }

    #[test]
    fn a_link_goes_by_its_last_part_or_its_host() {
        assert_eq!(Source::link(STREAM).unwrap().name(), "master.m3u8");
        assert_eq!(
            Source::link("https://example.com/films/").unwrap().name(),
            "films"
        );
        assert_eq!(
            Source::link("https://example.com").unwrap().name(),
            "example.com"
        );
    }

    #[test]
    fn a_source_is_kept_by_its_url() {
        let link = Source::link(STREAM).unwrap();
        assert_eq!(Source::kept(link.url()), Some(link));
        assert!(Source::kept("file:///data/clip.mp4").is_some());
        assert!(Source::kept("mailto:someone").is_none());
    }
}
