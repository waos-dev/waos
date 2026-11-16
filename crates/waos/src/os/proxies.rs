//! The proxy settings the system's own apps follow, where WaOS reads them itself: macOS's, from
//! SystemConfiguration's dynamic store (`SCDynamicStoreCopyProxies`), which every app on
//! CFNetwork reads, Safari among them. Windows's, in the registry's Internet Settings,
//! hyper-util's proxy matcher reads itself. Other systems have none beyond the environment's variables.
//!
//! The invariant: this module reads the settings and says nothing of what they mean for a
//! request, which is `network`'s.

/// A system's proxy settings: each proxy that is on, as `host:port`, the hosts and domains no
/// proxy is used for, and whether a name without a dot is reached without one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SystemProxies {
    pub(crate) http: Option<String>,
    pub(crate) https: Option<String>,
    /// A SOCKS proxy, for what neither of the others is set for.
    pub(crate) socks: Option<String>,
    /// The exceptions as the user wrote them: `*.local`, `169.254/16`, `localhost`.
    pub(crate) exceptions: Vec<String>,
    pub(crate) exclude_simple_hostnames: bool,
}

/// The system's proxy settings, which WaOS reads itself.
#[cfg(target_os = "macos")]
pub(crate) fn system_proxies() -> Option<SystemProxies> {
    Some(macos::system_proxies())
}

/// None: reqwest reads the registry's Internet Settings itself.
#[cfg(windows)]
pub(crate) fn system_proxies() -> Option<SystemProxies> {
    None
}

/// No settings beyond the environment's variables.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn system_proxies() -> Option<SystemProxies> {
    Some(SystemProxies::default())
}

#[cfg(target_os = "macos")]
mod macos {
    // The dynamic store's keys are statics the framework exports, and an exception is an
    // element of a Core Foundation array; reading both is what this module wraps.
    #![allow(unsafe_code)]

    use system_configuration::core_foundation::array::CFArray;
    use system_configuration::core_foundation::base::{CFType, TCFType};
    use system_configuration::core_foundation::dictionary::CFDictionary;
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesExceptionsList, kSCPropNetProxiesExcludeSimpleHostnames,
        kSCPropNetProxiesHTTPEnable, kSCPropNetProxiesHTTPPort, kSCPropNetProxiesHTTPProxy,
        kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort, kSCPropNetProxiesHTTPSProxy,
        kSCPropNetProxiesSOCKSEnable, kSCPropNetProxiesSOCKSPort, kSCPropNetProxiesSOCKSProxy,
    };

    use super::SystemProxies;

    type Settings = CFDictionary<CFString, CFType>;

    /// The dynamic store's proxy settings; none where the store cannot be read.
    pub(super) fn system_proxies() -> SystemProxies {
        let Some(settings) = SCDynamicStoreBuilder::new("WaOS")
            .build()
            .and_then(|store| store.get_proxies())
        else {
            return SystemProxies::default();
        };
        // SAFETY: each key is a constant `CFString` SystemConfiguration exports, never written.
        let keys = unsafe {
            [
                (
                    kSCPropNetProxiesHTTPEnable,
                    kSCPropNetProxiesHTTPProxy,
                    kSCPropNetProxiesHTTPPort,
                ),
                (
                    kSCPropNetProxiesHTTPSEnable,
                    kSCPropNetProxiesHTTPSProxy,
                    kSCPropNetProxiesHTTPSPort,
                ),
                (
                    kSCPropNetProxiesSOCKSEnable,
                    kSCPropNetProxiesSOCKSProxy,
                    kSCPropNetProxiesSOCKSPort,
                ),
            ]
        };
        let [http, https, socks] =
            keys.map(|(enable, host, port)| proxy(&settings, enable, host, port));
        // SAFETY: as above.
        let (exceptions, exclude_simple_hostnames) = unsafe {
            (
                kSCPropNetProxiesExceptionsList,
                kSCPropNetProxiesExcludeSimpleHostnames,
            )
        };
        SystemProxies {
            http,
            https,
            socks,
            exceptions: strings(&settings, exceptions),
            exclude_simple_hostnames: flag(&settings, exclude_simple_hostnames),
        }
    }

    /// The proxy `host` and `port` name, as `host:port`, when `enable` says it is on.
    fn proxy(
        settings: &Settings,
        enable: CFStringRef,
        host: CFStringRef,
        port: CFStringRef,
    ) -> Option<String> {
        if !flag(settings, enable) {
            return None;
        }
        let host = settings
            .find(host)
            .and_then(|host| host.downcast::<CFString>())?
            .to_string();
        let port = settings
            .find(port)
            .and_then(|port| port.downcast::<CFNumber>())
            .and_then(|port| port.to_i32());
        Some(match port {
            Some(port) => format!("{host}:{port}"),
            None => host,
        })
    }

    /// Whether the number at `key` is 1.
    fn flag(settings: &Settings, key: CFStringRef) -> bool {
        settings
            .find(key)
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i32())
            == Some(1)
    }

    /// The strings of the array at `key`; anything else in it left out.
    fn strings(settings: &Settings, key: CFStringRef) -> Vec<String> {
        let Some(array) = settings
            .find(key)
            .and_then(|value| value.downcast::<CFArray>())
        else {
            return Vec::new();
        };
        array
            .get_all_values()
            .into_iter()
            .filter_map(|element| {
                // SAFETY: an element of a Core Foundation array is a Core Foundation object,
                // which the array keeps alive while `array` is; it is retained for the wrap.
                let element = unsafe { CFType::wrap_under_get_rule(element) };
                element.downcast::<CFString>().map(|text| text.to_string())
            })
            .collect()
    }
}
