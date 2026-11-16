//! The names that are this computer, `localhost` and every name under it, answered by the engine
//! itself and never asked of the system's resolver, as browsers answer them, so they mean the
//! same on every system: macOS answers them as this computer, while Windows and Android ask the
//! DNS for a name under `localhost`, which finds none.
//!
//! The rules are Chromium's (`net/base/url_util.cc`, `IsLocalHostname`), which follow the
//! references a browser does:
//!
//! - RFC 6761, section 6.3: name resolution APIs and libraries should recognize `localhost.`
//!   and the names under it as special, always answer the loopback address for them, and not
//!   send them to a DNS server. draft-ietf-dnsop-let-localhost-be-localhost, section 3, makes
//!   both a must, and asks software that decides on security by these names to answer them
//!   itself rather than rely on the system's resolver.
//! - W3C Secure Contexts, "Is origin potentially trustworthy?": a host that is `localhost` or
//!   `localhost.`, or ends with `.localhost` or `.localhost.`, is this computer for a browser
//!   that answers them so.
//!
//! A name is such a name when, a final dot taken off, it is `localhost` or ends with
//! `.localhost`, in any case: `localhost`, `LocalHost.`, `app.localhost`. A name with a
//! `localhost` label elsewhere, `localhost.example`, is an ordinary name (the draft's section
//! 5.2). Such a name answers IPv6's loopback address, then IPv4's, in that order, as Chromium's
//! resolver does (`net/dns/host_resolver_manager.cc`, `ResolveLocalHostname`). Chromium then
//! keeps only the address families a lookup asked for; no lookup of a program's asks for one
//! (`wasi:sockets`', its HTTP's), so both are answered.
//!
//! "This computer" is the machine the program's network runs on, which is always this one: no
//! program's network is served from another machine yet.
//!
//! The invariant: the program's lookups, its HTTP requests and the proxies they go through all
//! ask [`localhost_addresses`] before any resolver, so no such name reaches the system's.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The name all the others are under.
const LOCALHOST: &str = "localhost";

/// The addresses `name` stands for when it is `localhost` or a name under it: IPv6's loopback
/// address, then IPv4's. None for any other name.
pub(crate) fn localhost_addresses(name: &str) -> Option<Vec<IpAddr>> {
    is_localhost(name).then(|| vec![Ipv6Addr::LOCALHOST.into(), Ipv4Addr::LOCALHOST.into()])
}

/// Whether `name`, a final dot taken off, is `localhost` or ends with `.localhost`, in any case.
fn is_localhost(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    let Some(start) = name.len().checked_sub(LOCALHOST.len()) else {
        return false;
    };
    let ends_with_localhost = name
        .get(start..)
        .is_some_and(|end| end.eq_ignore_ascii_case(LOCALHOST));
    ends_with_localhost && (start == 0 || name.as_bytes()[start - 1] == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_and_the_names_under_it_in_any_case_with_or_without_a_final_dot_are_this_computer()
    {
        for name in [
            "localhost",
            "localhost.",
            "LOCALHOST",
            "LocalHost.",
            "app.localhost",
            "app.localhost.",
            "a.b.APP.LOCALHOST",
            ".localhost",
        ] {
            assert!(is_localhost(name), "{name}");
        }
    }

    #[test]
    fn a_localhost_label_anywhere_else_is_an_ordinary_name() {
        for name in [
            "localhost.example",
            "localhost.example.",
            "app.localhost.example",
            "notlocalhost",
            "app-localhost",
            "localhost..",
            "localhos",
            "",
            ".",
            "example.com",
        ] {
            assert!(!is_localhost(name), "{name}");
        }
    }

    #[test]
    fn a_localhost_name_answers_ipv6_loopback_then_ipv4_loopback() {
        assert_eq!(
            localhost_addresses("app.localhost"),
            Some(vec![
                "::1".parse::<IpAddr>().unwrap(),
                "127.0.0.1".parse().unwrap()
            ])
        );
        assert_eq!(localhost_addresses("example.com"), None);
    }
}
