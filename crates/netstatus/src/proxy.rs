use crate::sc_store::{Dict, cf_string, cf_string_array};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use serde::Serialize;
use system_configuration::dynamic_store::SCDynamicStoreBuilder;

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ProxyEndpoint {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ProxyConfig {
    pub http: ProxyEndpoint,
    pub https: ProxyEndpoint,
    pub socks: ProxyEndpoint,
    pub pac_url: Option<String>,
    pub exceptions: Vec<String>,
}

fn cf_bool(dict: &Dict, key: &str) -> bool {
    dict.find(CFString::from(key))
        .and_then(|v| v.downcast::<CFBoolean>())
        .map(bool::from)
        .unwrap_or(false)
}

fn cf_port(dict: &Dict, key: &str) -> Option<u16> {
    dict.find(CFString::from(key))
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_i32())
        .and_then(|n| u16::try_from(n).ok())
}

/// Builds a `ProxyConfig` from the `CFDictionary` returned by
/// `SCDynamicStore::get_proxies`. Reading typed values straight out of the
/// dynamic store (rather than parsing `scutil --proxy`'s text dump) means
/// there is no `__SCOPED__`-style nested-block text to misparse — scoped,
/// per-interface overrides simply aren't present in `get_proxies()`'s
/// top-level keys at all.
pub(crate) fn proxy_config_from_dict(dict: &Dict) -> ProxyConfig {
    let endpoint = |enable_key: &str, host_key: &str, port_key: &str| ProxyEndpoint {
        enabled: cf_bool(dict, enable_key),
        host: cf_string(dict, host_key),
        port: cf_port(dict, port_key),
    };

    let exceptions = cf_string_array(dict, "ExceptionsList");

    ProxyConfig {
        http: endpoint("HTTPEnable", "HTTPProxy", "HTTPPort"),
        https: endpoint("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        socks: endpoint("SOCKSEnable", "SOCKSProxy", "SOCKSPort"),
        pac_url: if cf_bool(dict, "ProxyAutoConfigEnable") {
            cf_string(dict, "ProxyAutoConfigURLString")
        } else {
            None
        },
        exceptions,
    }
}

/// Reads the current proxy configuration from the System Configuration
/// dynamic store (`SCDynamicStoreCopyProxies`), the same data source
/// `scutil --proxy` prints as text.
pub fn proxy_config() -> ProxyConfig {
    let Some(store) = SCDynamicStoreBuilder::new("netcheck-proxy").build() else {
        return ProxyConfig::default();
    };
    match store.get_proxies() {
        Some(dict) => proxy_config_from_dict(&dict),
        None => ProxyConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;

    fn dict(pairs: &[(&str, CFType)]) -> Dict {
        let pairs: Vec<(CFString, CFType)> = pairs
            .iter()
            .map(|(k, v)| (CFString::from(*k), v.clone()))
            .collect();
        CFDictionary::from_CFType_pairs(&pairs)
    }

    #[test]
    fn no_proxy_configured() {
        let d = dict(&[]);
        let config = proxy_config_from_dict(&d);
        assert!(!config.http.enabled);
        assert!(!config.https.enabled);
        assert!(!config.socks.enabled);
        assert_eq!(config.pac_url, None);
        assert!(config.exceptions.is_empty());
    }

    #[test]
    fn http_proxy_enabled_with_host_and_port() {
        let d = dict(&[
            ("HTTPEnable", CFBoolean::from(true).as_CFType()),
            ("HTTPPort", CFNumber::from(8080).as_CFType()),
            ("HTTPProxy", CFString::from("proxy.example.com").as_CFType()),
        ]);
        let config = proxy_config_from_dict(&d);
        assert!(config.http.enabled);
        assert_eq!(config.http.host, Some("proxy.example.com".to_string()));
        assert_eq!(config.http.port, Some(8080));
    }

    #[test]
    fn pac_enabled_extracts_url() {
        let d = dict(&[
            ("ProxyAutoConfigEnable", CFBoolean::from(true).as_CFType()),
            (
                "ProxyAutoConfigURLString",
                CFString::from("http://example.com/proxy.pac").as_CFType(),
            ),
        ]);
        let config = proxy_config_from_dict(&d);
        assert_eq!(
            config.pac_url,
            Some("http://example.com/proxy.pac".to_string())
        );
    }

    #[test]
    fn pac_url_ignored_when_disabled() {
        let d = dict(&[
            ("ProxyAutoConfigEnable", CFBoolean::from(false).as_CFType()),
            (
                "ProxyAutoConfigURLString",
                CFString::from("http://example.com/proxy.pac").as_CFType(),
            ),
        ]);
        let config = proxy_config_from_dict(&d);
        assert_eq!(config.pac_url, None);
    }

    #[test]
    fn exceptions_list_parsed() {
        let exceptions =
            CFArray::from_CFTypes(&[CFString::from("*.local"), CFString::from("169.254/16")]);
        let d = dict(&[("ExceptionsList", exceptions.as_CFType())]);
        let config = proxy_config_from_dict(&d);
        assert_eq!(
            config.exceptions,
            vec!["*.local".to_string(), "169.254/16".to_string()]
        );
    }
}
