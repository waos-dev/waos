//! The proxies a program's requests may go through, read at its first request into one value
//! that both the route, which asks whether a proxy is in effect for a request (`route`), and the
//! connection made through a proxy (`connector`) ask, so the two agree.
//!
//! On macOS WaOS reads the system's settings itself (`os::system_proxies`): its HTTP, HTTPS and
//! SOCKS proxies, the exceptions the user listed, and whether a name without a dot is reached
//! without a proxy, so a host the user excepted is reached directly, as other apps reach it. The
//! environment's variables come first, as apps built with reqwest read them: `HTTP_PROXY`,
//! `HTTPS_PROXY` and `ALL_PROXY` over the system's proxies, and `NO_PROXY` beside the system's
//! exceptions. Other systems read them as hyper-util's matcher does: the environment's, and on
//! Windows the registry's Internet Settings.
//!
//! A SOCKS 5 proxy is asked for the host by its name (`socks5h`), whether the settings say `socks5`
//! or not, as an HTTP proxy is: the route has looked the name up already, and sends it through a
//! proxy only for internet answers, so the proxy looking it up again is the blind spot an HTTP
//! proxy has too, and no address it answered is handed over unchecked.
//!
//! The invariant: the route and the connection through a proxy ask the same matcher, so a request
//! the route sends through a proxy is sent through that proxy, and a name without a dot the
//! system reaches directly is reached directly by both.

use http::Uri;
use hyper_util::client::proxy::matcher::{Intercept, Matcher};

use crate::os::{SystemProxies, system_proxies};

/// The proxies a program's requests may go through.
pub(super) struct Proxies {
    matcher: Matcher,
    /// Whether a name without a dot is reached without a proxy.
    simple_hostnames_direct: bool,
}

/// Proxy settings in the form hyper-util's matcher takes: each proxy's URL, empty where none is
/// set, and the hosts no proxy is used for, as `NO_PROXY` lists them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ProxySettings {
    http: String,
    https: String,
    /// The proxy of either scheme where that scheme has none of its own.
    all: String,
    no: String,
    simple_hostnames_direct: bool,
}

/// The proxy settings of the environment's variables, as reqwest reads them; empty where unset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct EnvironmentProxies {
    all: String,
    http: String,
    https: String,
    no: String,
}

impl Proxies {
    /// The system's proxies: on macOS as WaOS reads them, beneath the environment's; elsewhere as
    /// hyper-util's matcher reads them.
    pub(super) fn system() -> Proxies {
        match system_proxies() {
            Some(system) => Proxies::listed(ProxySettings::of(EnvironmentProxies::read(), system)),
            None => Proxies {
                matcher: Matcher::from_system(),
                simple_hostnames_direct: false,
            },
        }
    }

    /// No proxy, whatever the machine has set.
    pub(super) fn none() -> Proxies {
        Proxies::listed(ProxySettings::default())
    }

    /// The proxies `settings` name.
    pub(super) fn listed(settings: ProxySettings) -> Proxies {
        Proxies {
            matcher: settings.matcher(),
            simple_hostnames_direct: settings.simple_hostnames_direct,
        }
    }

    /// The proxy a request for `uri` goes through, if one is in effect for it.
    pub(super) fn intercept(&self, uri: &Uri) -> Option<Intercept> {
        let host = uri.host()?;
        if self.simple_hostnames_direct && is_simple_hostname(host) {
            return None;
        }
        self.matcher.intercept(uri)
    }
}

impl ProxySettings {
    /// Every request through the proxy at `url`, whatever its scheme.
    #[cfg(test)]
    pub(super) fn through(url: &str) -> ProxySettings {
        ProxySettings {
            all: named_to_the_proxy(url.to_owned()),
            ..ProxySettings::default()
        }
    }

    /// `environment`'s settings over `system`'s: a proxy the environment names over the system's
    /// for its scheme, and the hosts either excepts.
    fn of(environment: EnvironmentProxies, system: SystemProxies) -> ProxySettings {
        let exceptions = system
            .exceptions
            .iter()
            .map(|exception| no_proxy_entry(exception));
        let no = std::iter::once(environment.no)
            .chain(exceptions)
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        ProxySettings {
            http: or_system(environment.http, system.http, "http"),
            https: or_system(environment.https, system.https, "http"),
            all: named_to_the_proxy(or_system(environment.all, system.socks, "socks5h")),
            no,
            simple_hostnames_direct: system.exclude_simple_hostnames,
        }
    }

    fn matcher(&self) -> Matcher {
        Matcher::builder()
            .http(self.http.as_str())
            .https(self.https.as_str())
            .all(self.all.as_str())
            .no(self.no.as_str())
            .build()
    }
}

impl EnvironmentProxies {
    /// The variables reqwest reads, each in capitals first.
    fn read() -> EnvironmentProxies {
        EnvironmentProxies {
            all: variable(["ALL_PROXY", "all_proxy"]),
            http: variable(["HTTP_PROXY", "http_proxy"]),
            https: variable(["HTTPS_PROXY", "https_proxy"]),
            no: variable(["NO_PROXY", "no_proxy"]),
        }
    }
}

/// The first of `names` set in the environment; empty when none is.
fn variable(names: [&str; 2]) -> String {
    names
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
        .unwrap_or_default()
}

/// `environment`'s proxy, or else `system`'s, a proxy of `scheme` at its `host:port`.
fn or_system(environment: String, system: Option<String>, scheme: &str) -> String {
    match system {
        Some(host_and_port) if environment.is_empty() => format!("{scheme}://{host_and_port}"),
        _ => environment,
    }
}

/// `url`, a SOCKS 5 proxy's asked for a host by its name (`socks5h`) rather than by an address
/// looked up first.
fn named_to_the_proxy(url: String) -> String {
    match url.strip_prefix("socks5://") {
        Some(rest) => format!("socks5h://{rest}"),
        None => url,
    }
}

/// A system exception in `NO_PROXY`'s form, which matches a domain with its subdomains and an
/// IPv4 network by its whole address: `*.local` as `.local`, `169.254/16` as `169.254.0.0/16`.
fn no_proxy_entry(exception: &str) -> String {
    let exception = exception.trim();
    if let Some(domain) = exception.strip_prefix("*.") {
        return format!(".{domain}");
    }
    whole_ipv4_network(exception).unwrap_or_else(|| exception.to_owned())
}

/// An IPv4 network written with fewer than four numbers, `169.254/16`, with the rest as zeros.
fn whole_ipv4_network(exception: &str) -> Option<String> {
    let (address, prefix) = exception.split_once('/')?;
    let numbers: Vec<u8> = address
        .split('.')
        .map(|number| number.parse().ok())
        .collect::<Option<_>>()?;
    if numbers.is_empty() || numbers.len() >= 4 {
        return None;
    }
    let mut octets = [0; 4];
    octets[..numbers.len()].copy_from_slice(&numbers);
    let [a, b, c, d] = octets;
    Some(format!("{a}.{b}.{c}.{d}/{prefix}"))
}

/// Whether `host` is a name without a dot, which macOS calls a simple hostname: no address, and
/// no domain.
fn is_simple_hostname(host: &str) -> bool {
    !host.contains('.') && !host.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(text: &str) -> Uri {
        text.parse().unwrap()
    }

    /// The proxy a request for `url` goes through, if any.
    fn proxy_of(proxies: &Proxies, url: &str) -> Option<String> {
        proxies
            .intercept(&uri(url))
            .map(|intercept| intercept.uri().to_string())
    }

    /// The system's settings of a Mac with an HTTP proxy and some exceptions.
    fn system() -> SystemProxies {
        SystemProxies {
            http: Some("proxy.test:8080".into()),
            https: Some("proxy.test:8443".into()),
            socks: None,
            exceptions: vec![
                "*.local".into(),
                "169.254/16".into(),
                "10.0.0.0/8".into(),
                "*.excepted.example".into(),
                "localhost".into(),
            ],
            exclude_simple_hostnames: false,
        }
    }

    #[test]
    fn a_host_the_system_excepts_is_reached_directly_and_any_other_through_its_proxy() {
        let proxies = Proxies::listed(ProxySettings::of(EnvironmentProxies::default(), system()));
        for excepted in [
            "http://api.excepted.example/",
            "http://printer.local/",
            "http://169.254.10.20/",
            "http://10.1.2.3/",
            "http://localhost:3000/",
        ] {
            assert_eq!(proxy_of(&proxies, excepted), None, "{excepted}");
        }
        assert_eq!(
            proxy_of(&proxies, "http://example.org/").as_deref(),
            Some("http://proxy.test:8080/")
        );
        assert_eq!(
            proxy_of(&proxies, "https://example.org/").as_deref(),
            Some("http://proxy.test:8443/")
        );
    }

    #[test]
    fn a_name_without_a_dot_is_reached_directly_where_the_system_says_so() {
        let excluding = SystemProxies {
            exclude_simple_hostnames: true,
            ..system()
        };
        let proxies = Proxies::listed(ProxySettings::of(EnvironmentProxies::default(), excluding));
        assert_eq!(proxy_of(&proxies, "http://intranet/"), None);
        assert!(proxy_of(&proxies, "http://intranet.example/").is_some());
        let proxies = Proxies::listed(ProxySettings::of(EnvironmentProxies::default(), system()));
        assert!(proxy_of(&proxies, "http://intranet/").is_some());
    }

    #[test]
    fn the_environment_comes_before_the_system_and_its_no_proxy_still_counts() {
        let environment = EnvironmentProxies {
            http: "http://environment.test:1".into(),
            no: "environment-excepted.example".into(),
            ..EnvironmentProxies::default()
        };
        let proxies = Proxies::listed(ProxySettings::of(environment, system()));
        assert_eq!(
            proxy_of(&proxies, "http://example.org/").as_deref(),
            Some("http://environment.test:1/")
        );
        assert_eq!(
            proxy_of(&proxies, "https://example.org/").as_deref(),
            Some("http://proxy.test:8443/"),
            "the system's, for the scheme the environment names none for"
        );
        assert_eq!(
            proxy_of(&proxies, "http://www.environment-excepted.example/"),
            None
        );
        assert_eq!(proxy_of(&proxies, "http://a.excepted.example/"), None);
    }

    #[test]
    fn a_socks_proxy_is_asked_for_names_whether_the_system_or_the_environment_names_it() {
        let socks = SystemProxies {
            http: None,
            https: None,
            socks: Some("socks.test:1080".into()),
            ..system()
        };
        let proxies = Proxies::listed(ProxySettings::of(EnvironmentProxies::default(), socks));
        assert_eq!(
            proxy_of(&proxies, "https://example.org/").as_deref(),
            Some("socks5h://socks.test:1080/")
        );
        let environment = EnvironmentProxies {
            all: "socks5://environment.test:1080".into(),
            ..EnvironmentProxies::default()
        };
        let proxies = Proxies::listed(ProxySettings::of(environment, system()));
        assert_eq!(
            proxy_of(&proxies, "http://example.org/").as_deref(),
            Some("http://proxy.test:8080/"),
            "the system's HTTP proxy over the environment's of either scheme, as reqwest reads them"
        );
        let only_socks = EnvironmentProxies {
            all: "socks5://environment.test:1080".into(),
            ..EnvironmentProxies::default()
        };
        let proxies = Proxies::listed(ProxySettings::of(only_socks, SystemProxies::default()));
        assert_eq!(
            proxy_of(&proxies, "http://example.org/").as_deref(),
            Some("socks5h://environment.test:1080/")
        );
    }

    #[test]
    fn no_proxy_at_all_is_in_effect_for_nothing() {
        assert_eq!(proxy_of(&Proxies::none(), "http://example.org/"), None);
    }
}
