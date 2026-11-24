//! A package source: where an `index.json` of programs is read from (the design's decision 45).
//! Either an index at its own URL, or a GitHub repository, whose latest release carries the index
//! and its packages as assets, read at `…/releases/latest/download/index.json` with no GitHub API.
//!
//! The invariant a later editor will break: an index is read over HTTPS, or over plain HTTP only
//! from this machine, since the index is not signed and says which package a first install
//! trusts; a source's packages are found relative to the index's own URL.

use std::fmt;
use std::str::FromStr;

use url::{Host, Url};

use crate::SourceError;

/// Where GitHub serves the assets of a repository's latest release, after the repository's path.
const LATEST_RELEASE_ASSETS: &str = "releases/latest/download/";

/// The index's name: the asset a GitHub source's latest release carries.
const INDEX: &str = "index.json";

/// A package source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// An index at its own URL.
    Index(Url),
    /// A GitHub repository, `github.com/owner/repository`.
    GitHub { owner: String, repository: String },
}

impl Source {
    /// Where the source's index is read.
    pub fn index_url(&self) -> Url {
        match self {
            Source::Index(url) => url.clone(),
            Source::GitHub { owner, repository } => {
                let url = format!(
                    "https://github.com/{owner}/{repository}/{LATEST_RELEASE_ASSETS}{INDEX}"
                );
                Url::parse(&url).expect("an owner and a repository make a URL")
            }
        }
    }

    /// Where the package the index lists as `package` is: that, relative to the index's URL.
    pub(crate) fn package_url(&self, package: &str) -> Result<Url, SourceError> {
        self.index_url()
            .join(package)
            .map_err(|error| SourceError::PackageUrl {
                package: package.to_owned(),
                error,
            })
    }

    /// Where the picture the index lists as `picture` is: that, relative to the index's URL, and
    /// on the index's own server, since a source's word is no leave to read from anywhere: a
    /// picture's address could otherwise be a device of the user's own network.
    pub fn picture_url(&self, picture: &str) -> Result<Url, SourceError> {
        let index = self.index_url();
        let url = self.package_url(picture)?;
        if url.origin() != index.origin() {
            return Err(SourceError::PictureElsewhere { url });
        }
        Ok(url)
    }
}

/// A source as the user types it: `github.com/owner/repository`, the repository's
/// `https://github.com/owner/repository`, or the `https://` URL of an index; `http://` only for an
/// index on this machine.
impl FromStr for Source {
    type Err = SourceError;

    fn from_str(text: &str) -> Result<Source, SourceError> {
        let text = text.trim();
        let not_a_source = |reason| SourceError::NotASource {
            text: text.to_owned(),
            reason,
        };
        if let Some(path) = text.strip_prefix("github.com/") {
            return github_repository(path).ok_or_else(|| not_a_source(NOT_A_REPOSITORY));
        }
        let url = Url::parse(text).map_err(|_| not_a_source(NEITHER))?;
        if url.scheme() == "https" && url.host_str() == Some("github.com") {
            return github_repository(url.path().trim_start_matches('/'))
                .map_or_else(|| index_at(url), Ok);
        }
        match url.scheme() {
            "https" => index_at(url),
            "http" if is_this_machine(&url) => index_at(url),
            "http" => Err(not_a_source(PLAIN_HTTP)),
            _ => Err(not_a_source(NEITHER)),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Index(url) => f.write_str(url.as_str()),
            Source::GitHub { owner, repository } => write!(f, "github.com/{owner}/{repository}"),
        }
    }
}

const NEITHER: &str =
    "a source is an index's https:// URL, or a GitHub repository as github.com/owner/repository";
const NOT_A_REPOSITORY: &str =
    "a GitHub repository is github.com/owner/repository, each of letters, digits, '-', '_' and '.'";
const PLAIN_HTTP: &str = "an index is read over https://, or over http:// only from this machine";

fn index_at(url: Url) -> Result<Source, SourceError> {
    Ok(Source::Index(url))
}

/// The repository `path` names, `owner/repository` with an optional `/` after it; none when it
/// names anything else, such as an asset's URL.
fn github_repository(path: &str) -> Option<Source> {
    let path = path.strip_suffix('/').unwrap_or(path);
    let (owner, repository) = path.split_once('/')?;
    (is_github_name(owner) && is_github_name(repository)).then(|| Source::GitHub {
        owner: owner.to_owned(),
        repository: repository.to_owned(),
    })
}

/// Whether `name` is an owner's or a repository's name as GitHub allows them.
fn is_github_name(name: &str) -> bool {
    let allowed =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.');
    !name.is_empty() && name != "." && name != ".." && name.chars().all(allowed)
}

/// Whether `url` is served from this machine, which plain HTTP crosses no network to reach.
fn is_this_machine(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain == "localhost",
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(text: &str) -> Source {
        text.parse().unwrap()
    }

    fn reason(text: &str) -> &'static str {
        match text.parse::<Source>().unwrap_err() {
            SourceError::NotASource { reason, .. } => reason,
            other => panic!("{other}"),
        }
    }

    #[test]
    fn a_github_repository_is_read_from_its_latest_release() {
        let expected = Source::GitHub {
            owner: "example".to_owned(),
            repository: "triangle.rs".to_owned(),
        };
        for text in [
            "github.com/example/triangle.rs",
            "https://github.com/example/triangle.rs",
            "https://github.com/example/triangle.rs/",
            " github.com/example/triangle.rs\n",
        ] {
            assert_eq!(source(text), expected, "{text}");
        }
        assert_eq!(
            expected.index_url().as_str(),
            "https://github.com/example/triangle.rs/releases/latest/download/index.json"
        );
        assert_eq!(expected.to_string(), "github.com/example/triangle.rs");
    }

    #[test]
    fn a_github_sources_packages_are_assets_of_the_same_release() {
        let github = source("github.com/example/triangle");
        assert_eq!(
            github
                .package_url("dev.example.triangle-1.2.0.wapk")
                .unwrap()
                .as_str(),
            "https://github.com/example/triangle/releases/latest/download/dev.example.triangle-1.2.0.wapk"
        );
    }

    #[test]
    fn an_index_is_read_at_its_url_and_its_packages_beside_it() {
        let index = source("https://example.com/waos/index.json");
        assert_eq!(index, Source::Index(index.index_url()));
        assert_eq!(index.to_string(), "https://example.com/waos/index.json");
        assert_eq!(
            index.package_url("triangle-1.2.0.wapk").unwrap().as_str(),
            "https://example.com/waos/triangle-1.2.0.wapk"
        );
        assert_eq!(
            index
                .package_url("https://cdn.example.com/triangle.wapk")
                .unwrap()
                .as_str(),
            "https://cdn.example.com/triangle.wapk"
        );
    }

    #[test]
    fn a_github_url_that_is_not_a_repository_is_an_index() {
        let asset = "https://github.com/example/triangle/releases/download/v1/index.json";
        assert_eq!(source(asset), Source::Index(Url::parse(asset).unwrap()));
    }

    #[test]
    fn plain_http_is_read_only_from_this_machine() {
        for local in [
            "http://localhost:8000/index.json",
            "http://127.0.0.1:8000/index.json",
            "http://[::1]:8000/index.json",
        ] {
            assert!(matches!(source(local), Source::Index(_)), "{local}");
        }
        assert_eq!(reason("http://example.com/index.json"), PLAIN_HTTP);
    }

    #[test]
    fn anything_else_is_not_a_source() {
        for text in [
            "",
            "triangle",
            "ftp://example.com/index.json",
            "file:///index.json",
        ] {
            assert_eq!(reason(text), NEITHER, "{text}");
        }
        for text in [
            "github.com/example",
            "github.com/example/triangle/releases",
            "github.com/example/../triangle",
            "github.com/exa mple/triangle",
        ] {
            assert_eq!(reason(text), NOT_A_REPOSITORY, "{text}");
        }
    }

    #[test]
    fn a_source_reads_back_from_what_it_shows() {
        for text in [
            "github.com/example/triangle",
            "https://example.com/waos/index.json",
            "http://localhost:8000/index.json",
        ] {
            assert_eq!(source(&source(text).to_string()), source(text));
        }
    }
}
