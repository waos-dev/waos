//! A feed's address as the user gives it, made the address its document is fetched from: typed or
//! pasted, with or without its scheme, or as a `feed:` link.

use url::Url;

/// Why what the user gave is no feed's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BadAddress {
    Empty,
    /// It is no web address.
    NotAnAddress,
    /// It is an address of something other than the web: a file, or mail.
    NotTheWeb,
}

impl BadAddress {
    pub(crate) fn said(self) -> &'static str {
        match self {
            BadAddress::Empty => "Enter the feed’s address.",
            BadAddress::NotAnAddress => "That is not a web address.",
            BadAddress::NotTheWeb => "A feed’s address begins with https:// or http://.",
        }
    }
}

/// The address `given` names: `https` where it names no scheme, and a `feed:` link as the web
/// address it stands for.
pub(crate) fn feed_address(given: &str) -> Result<Url, BadAddress> {
    let given = given.trim();
    if given.is_empty() {
        return Err(BadAddress::Empty);
    }
    let given = given
        .strip_prefix("feed://")
        .or_else(|| given.strip_prefix("feed:"))
        .unwrap_or(given);
    let with_scheme = if given.contains("://") {
        given.to_owned()
    } else {
        format!("https://{given}")
    };
    let url = Url::parse(&with_scheme).map_err(|_| BadAddress::NotAnAddress)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(BadAddress::NotTheWeb);
    }
    let names_a_site = url.host_str().is_some_and(|host| host.contains('.'));
    if names_a_site {
        Ok(url)
    } else {
        Err(BadAddress::NotAnAddress)
    }
}

/// What a feed is called until its document says: its address's host, without `www.`.
pub(crate) fn host_name(address: &Url) -> String {
    let host = address.host_str().unwrap_or_default();
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_takes_https_where_it_names_no_scheme() {
        for (given, address) in [
            (
                "blog.rust-lang.org/feed.xml",
                "https://blog.rust-lang.org/feed.xml",
            ),
            ("  https://example.org/rss  ", "https://example.org/rss"),
            ("http://example.org/rss", "http://example.org/rss"),
            ("feed://example.org/rss", "https://example.org/rss"),
            ("feed:https://example.org/rss", "https://example.org/rss"),
        ] {
            assert_eq!(feed_address(given).unwrap().as_str(), address, "{given}");
        }
    }

    #[test]
    fn what_is_no_web_address_is_refused() {
        assert_eq!(feed_address("  "), Err(BadAddress::Empty));
        assert_eq!(feed_address("news"), Err(BadAddress::NotAnAddress));
        assert_eq!(feed_address("two words"), Err(BadAddress::NotAnAddress));
        assert_eq!(
            feed_address("file:///etc/passwd"),
            Err(BadAddress::NotTheWeb)
        );
        assert_eq!(
            feed_address("ftp://example.org/feed"),
            Err(BadAddress::NotTheWeb)
        );
    }

    #[test]
    fn a_feed_is_first_called_by_its_host() {
        let address = feed_address("https://www.nasa.gov/feed/").unwrap();
        assert_eq!(host_name(&address), "nasa.gov");
    }
}
