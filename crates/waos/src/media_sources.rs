//! What a program's media source names, and the URL the system is handed for it, by a player or
//! by the reading of media without playing it: a link of `http` or `https`, which the system
//! fetches itself once the program's own lookup of its host lets the program reach it (decision 8
//! of the media proposal); or an absolute path inside the program, which its mounts map to where
//! the file is on the machine, given as its `file:` URL.
//!
//! The invariant: a URL leaves here only for a link whose host the program may reach, or for a
//! file inside its mounts. A link of another scheme is no source, since the network rules say
//! nothing of it, and a `file:` link would name a file outside the mounts.

use std::path::{Path, PathBuf};

use url::Url;

use crate::access::Cause;
use crate::files::ProgramFiles;
use crate::network::{LookupError, ProgramNetwork};

/// The schemes of the links media is opened on: the web's, whose hosts the network rules judge.
const LINK_SCHEMES: [&str; 2] = ["http", "https"];

/// What a source names.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Named {
    /// A link the system fetches.
    Link(Url),
    /// A path inside the program.
    Path(PathBuf),
}

/// What `source` names: an absolute path inside the program, or a link of `http` or `https` with
/// a host; why it is no source otherwise.
pub(crate) fn named(source: &str) -> Result<Named, String> {
    if source.starts_with('/') {
        return Ok(Named::Path(PathBuf::from(source)));
    }
    match Url::parse(source) {
        Ok(link) if LINK_SCHEMES.contains(&link.scheme()) && link.host().is_some() => {
            Ok(Named::Link(link))
        }
        _ => Err(format!(
            "{source:?} is neither a path inside the program nor an http or https link"
        )),
    }
}

/// The `file:` URL of where the program's `path` is on the machine; why not for a path outside
/// the program's files.
pub(crate) fn file_url(files: &mut ProgramFiles, path: &Path) -> Result<String, String> {
    let machine_path = files
        .machine_path(path)
        .ok_or_else(|| format!("no file at {} among the program's", path.display()))?;
    Url::from_file_path(&machine_path)
        .map(String::from)
        .map_err(|()| format!("{} has no file URL", machine_path.display()))
}

/// Whether the program may reach `link`'s host, looked up as the host of a request of its own is,
/// told as a lookup the player caused; why not otherwise. Blocks while the system resolves.
pub(crate) fn check_link(network: &ProgramNetwork, link: &Url) -> Result<(), String> {
    let host = link
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']');
    match network.reachable(host, Cause::Player) {
        Ok(_addresses) => Ok(()),
        Err(LookupError::Refused(_)) => Err(format!("the program may not reach {host}")),
        Err(LookupError::Unresolvable) => Err(format!("no address found for {host}")),
        Err(LookupError::Invalid) => Err(format!("{host} is no host name")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::access::ProgramAccess;
    use crate::network::test_grants::{TestGrants, off};

    #[test]
    fn a_source_is_an_absolute_path_or_a_web_link() {
        assert_eq!(
            named("/data/a b.mp4"),
            Ok(Named::Path(PathBuf::from("/data/a b.mp4")))
        );
        let link = "https://example.com/master.m3u8";
        assert_eq!(named(link), Ok(Named::Link(Url::parse(link).unwrap())));
        assert!(matches!(
            named("HTTP://example.com/a.mp4"),
            Ok(Named::Link(_))
        ));
        for text in [
            "file:///etc/hosts",
            "ftp://example.com/a.mp4",
            "data/a.mp4",
            "",
            "https://",
        ] {
            assert!(named(text).is_err(), "{text:?}");
        }
    }

    /// A network that is off, with the local network as `local`, every name resolving to this
    /// computer.
    fn network(local: bool) -> ProgramNetwork {
        ProgramNetwork::resolving_with(
            ProgramAccess::new(TestGrants::new(off(), local)),
            Arc::new(|_name| Ok(vec!["127.0.0.1".parse().unwrap()])),
        )
    }

    #[test]
    fn a_links_host_is_looked_up_as_a_request_of_the_programs_is() {
        let local = Url::parse("http://media.localhost:8080/a.m3u8").unwrap();
        let internet = Url::parse("https://192.0.2.1/a.m3u8").unwrap();
        let bracketed = Url::parse("http://[::1]/a.m3u8").unwrap();
        assert_eq!(check_link(&network(true), &local), Ok(()));
        assert_eq!(check_link(&network(true), &bracketed), Ok(()));
        assert_eq!(
            check_link(&network(true), &internet),
            Err("the program may not reach 192.0.2.1".to_owned())
        );
        assert_eq!(
            check_link(&network(false), &local),
            Err("the program may not reach media.localhost".to_owned()),
            "a host on this computer needs the local-network switch"
        );
    }
}
