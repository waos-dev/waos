//! A package source: where apps are listed. Either a folder on the web, which holds an
//! `index.json` with the packages and the pictures it lists beside it, or a GitHub repository,
//! whose latest release carries the same as its assets, read at `…/releases/latest/download/`
//! with no GitHub API. A website is a source by its address alone, as `waos.app`: its folder is
//! `/.well-known/waos/`, where a site says what it offers one kind of client (RFC 8615).
//!
//! The invariant a later editor will break: a source is read over HTTPS, or over plain HTTP only
//! from this machine, since the index is not signed and says which package a first install
//! trusts; a source's packages are found relative to the index's own URL; and what a source
//! shows as its address reads back as the same source, since that is how the list of sources is
//! kept.

use std::fmt;
use std::str::FromStr;

use url::{Host, Url};

use crate::SourceError;

/// Where GitHub serves the assets of a repository's latest release, after the repository's path.
const LATEST_RELEASE_ASSETS: &str = "releases/latest/download/";

/// The index's name in a source's folder, and among the assets of a GitHub source's release.
const INDEX: &str = "index.json";

/// The folder of a website that is a source by its address alone.
const SITE_FOLDER: &str = "/.well-known/waos/";

/// A package source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A folder on the web, by its URL, which ends in `/`: a website's [`SITE_FOLDER`], or
    /// any other its publisher names.
    Folder(Url),
    /// A GitHub repository, `github.com/owner/repository`.
    GitHub { owner: String, repository: String },
}

impl Source {
    /// Where the source's index is read.
    pub fn index_url(&self) -> Url {
        match self {
            Source::Folder(folder) => folder.join(INDEX).expect("a folder's URL holds a file"),
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

    /// The picture that stands for where the source is, as [`picture_url`](Self::picture_url)
    /// reads one, for a store to show where the source lists no icon of its own: a website's
    /// touch icon, the PNG Safari reads at a site's root, and the picture of a GitHub
    /// repository's owner. A place that has none answers that it is not found.
    pub fn site_picture(&self) -> String {
        match self {
            Source::Folder(_) => "/apple-touch-icon.png".to_owned(),
            Source::GitHub { owner, .. } => format!("/{owner}.png"),
        }
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

/// A source as the user types it: a website's address, as `waos.app`; a folder's, as
/// `example.com/apps`; or a GitHub repository, as `github.com/owner/repository` or its
/// `https://` URL. An address with no scheme is read over `https://`; `http://` only for a source
/// on this machine.
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
        let address = if text.contains("://") {
            text.to_owned()
        } else {
            format!("https://{text}")
        };
        let url = Url::parse(&address).map_err(|_| not_a_source(NEITHER))?;
        if url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && let Some(repository) = github_repository(url.path().trim_start_matches('/'))
        {
            return Ok(repository);
        }
        match url.scheme() {
            "https" if names_a_server(&url) => Ok(folder_at(url)),
            "http" if is_this_machine(&url) => Ok(folder_at(url)),
            "http" => Err(not_a_source(PLAIN_HTTP)),
            _ => Err(not_a_source(NEITHER)),
        }
    }
}

/// The address a person types for the source, the shortest that reads back as it: a website's
/// as `waos.app`, a folder's as `example.com/apps`, with `http://` before the one read so.
impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Folder(folder) => {
                if folder.scheme() != "https" {
                    write!(f, "{}://", folder.scheme())?;
                }
                f.write_str(folder.host_str().unwrap_or_default())?;
                if let Some(port) = folder.port() {
                    write!(f, ":{port}")?;
                }
                if folder.path() != SITE_FOLDER {
                    f.write_str(folder.path().trim_end_matches('/'))?;
                }
                Ok(())
            }
            Source::GitHub { owner, repository } => write!(f, "github.com/{owner}/{repository}"),
        }
    }
}

const NEITHER: &str = "a source is a website's address, as waos.app, or a GitHub repository as github.com/owner/repository";
const NOT_A_REPOSITORY: &str =
    "a GitHub repository is github.com/owner/repository, each of letters, digits, '-', '_' and '.'";
const PLAIN_HTTP: &str = "a source is read over https://, or over http:// only from this machine";

/// The source `url` names: the folder at its path, or the website's own where it has none. What
/// follows the path, a query or a fragment, names no folder and is left out.
fn folder_at(mut url: Url) -> Source {
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/');
    let folder = if path.is_empty() {
        SITE_FOLDER.to_owned()
    } else {
        format!("{path}/")
    };
    url.set_path(&folder);
    Source::Folder(url)
}

/// Whether `url` names a server as an address on the web does: a domain of more than one label,
/// `localhost`, or an IP address; a lone word is something mistyped.
fn names_a_server(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain == "localhost" || domain.contains('.'),
        Some(Host::Ipv4(_) | Host::Ipv6(_)) => true,
        None => false,
    }
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
    fn a_website_is_a_source_by_its_address_read_at_its_well_known_folder() {
        for text in [
            "example.com",
            "example.com/",
            "https://example.com",
            "https://example.com/?from=a-link#top",
        ] {
            let site = source(text);
            assert_eq!(
                site.index_url().as_str(),
                "https://example.com/.well-known/waos/index.json",
                "{text}"
            );
            assert_eq!(site.to_string(), "example.com", "{text}");
        }
    }

    #[test]
    fn a_folder_is_read_at_its_url_with_its_packages_beside_its_index() {
        for text in [
            "example.com/apps",
            "https://example.com/apps/",
            "https://example.com/apps",
        ] {
            let folder = source(text);
            assert_eq!(
                folder.index_url().as_str(),
                "https://example.com/apps/index.json",
                "{text}"
            );
            assert_eq!(folder.to_string(), "example.com/apps", "{text}");
        }
        let folder = source("example.com/apps");
        assert_eq!(
            folder.package_url("triangle-1.2.0.wapk").unwrap().as_str(),
            "https://example.com/apps/triangle-1.2.0.wapk"
        );
        assert_eq!(
            folder
                .package_url("https://cdn.example.com/triangle.wapk")
                .unwrap()
                .as_str(),
            "https://cdn.example.com/triangle.wapk"
        );
    }

    #[test]
    fn a_sources_site_has_a_picture_on_the_sources_own_server() {
        let picture_of = |text: &str| {
            let source = source(text);
            source.picture_url(&source.site_picture()).unwrap()
        };
        assert_eq!(
            picture_of("example.com/apps").as_str(),
            "https://example.com/apple-touch-icon.png"
        );
        assert_eq!(
            picture_of("github.com/example/triangle").as_str(),
            "https://github.com/example.png"
        );
    }

    #[test]
    fn a_github_url_that_is_not_a_repository_is_a_folder() {
        let release = source("https://github.com/example/triangle/releases/download/v1");
        assert_eq!(
            release.index_url().as_str(),
            "https://github.com/example/triangle/releases/download/v1/index.json"
        );
    }

    #[test]
    fn plain_http_is_read_only_from_this_machine() {
        for (local, shown) in [
            ("http://localhost:8000", "http://localhost:8000"),
            ("http://127.0.0.1:8000/", "http://127.0.0.1:8000"),
            ("http://[::1]:8000/apps", "http://[::1]:8000/apps"),
        ] {
            assert_eq!(source(local).to_string(), shown);
        }
        assert_eq!(
            source("http://localhost:8000").index_url().as_str(),
            "http://localhost:8000/.well-known/waos/index.json"
        );
        assert_eq!(reason("http://example.com"), PLAIN_HTTP);
    }

    #[test]
    fn anything_else_is_not_a_source() {
        for text in ["", "triangle", "ftp://example.com", "file:///apps"] {
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
            "example.com",
            "example.com/apps",
            "example.com:8443/apps",
            "http://localhost:8000",
            "http://localhost:8000/apps",
        ] {
            assert_eq!(source(text).to_string(), text);
            assert_eq!(source(&source(text).to_string()), source(text));
        }
    }
}
