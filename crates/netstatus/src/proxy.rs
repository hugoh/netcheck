use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProxyEndpoint {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProxyConfig {
    pub http: ProxyEndpoint,
    pub https: ProxyEndpoint,
    pub socks: ProxyEndpoint,
    pub pac_url: Option<String>,
    pub exceptions: Vec<String>,
}

pub(crate) fn parse_proxy_config(text: &str) -> ProxyConfig {
    let mut values: HashMap<&str, &str> = HashMap::new();
    let mut exceptions = Vec::new();
    let mut in_exceptions = false;
    // Depth of nested `<dictionary> {` / `<array> {` blocks (other than
    // ExceptionsList) currently being skipped, e.g. `scutil --proxy`'s
    // per-interface `__SCOPED__` dictionary. Their key/value lines must not
    // fall through into the flat `values` map, or a scoped override could
    // silently clobber the top-level setting it's meant to be independent
    // from.
    let mut skip_depth: u32 = 0;

    for line in text.lines() {
        let trimmed = line.trim();

        if skip_depth > 0 {
            if trimmed.ends_with('{') {
                skip_depth += 1;
            } else if trimmed == "}" {
                skip_depth -= 1;
            }
            continue;
        }

        if in_exceptions {
            if trimmed == "}" {
                in_exceptions = false;
            } else if let Some((_, value)) = trimmed.split_once(" : ") {
                exceptions.push(value.trim().to_string());
            }
            continue;
        }

        if trimmed.ends_with("<array> {") || trimmed.ends_with("<dictionary> {") {
            let key = trimmed.split_once(" : ").map(|(key, _)| key.trim());
            if key == Some("ExceptionsList") {
                in_exceptions = true;
            } else if key.is_some() {
                // A nested block under a key, e.g. `__SCOPED__ : <dictionary>
                // {`. The top-level `<dictionary> {` has no key and is left
                // unskipped so its contents are parsed normally.
                skip_depth = 1;
            }
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(" : ") {
            values.insert(key.trim(), value.trim());
        }
    }

    let endpoint = |enable_key: &str, host_key: &str, port_key: &str| ProxyEndpoint {
        enabled: values.get(enable_key) == Some(&"1"),
        host: values.get(host_key).map(|s| s.to_string()),
        port: values.get(port_key).and_then(|s| s.parse().ok()),
    };

    ProxyConfig {
        http: endpoint("HTTPEnable", "HTTPProxy", "HTTPPort"),
        https: endpoint("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        socks: endpoint("SOCKSEnable", "SOCKSProxy", "SOCKSPort"),
        pac_url: if values.get("ProxyAutoConfigEnable") == Some(&"1") {
            values
                .get("ProxyAutoConfigURLString")
                .map(|s| s.to_string())
        } else {
            None
        },
        exceptions,
    }
}

/// Runs `scutil --proxy` and parses its dictionary dump into a `ProxyConfig`.
pub fn proxy_config() -> ProxyConfig {
    let output = Command::new("scutil")
        .arg("--proxy")
        .output()
        .expect("scutil --proxy should be runnable on macOS");
    let text = String::from_utf8_lossy(&output.stdout);
    parse_proxy_config(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_proxy_configured() {
        let text = "<dictionary> {\n}\n";
        let config = parse_proxy_config(text);
        assert!(!config.http.enabled);
        assert!(!config.https.enabled);
        assert!(!config.socks.enabled);
        assert_eq!(config.pac_url, None);
        assert!(config.exceptions.is_empty());
    }

    #[test]
    fn http_proxy_enabled_with_host_and_port() {
        let text = "<dictionary> {\n  \
                     HTTPEnable : 1\n  \
                     HTTPPort : 8080\n  \
                     HTTPProxy : proxy.example.com\n\
                     }\n";
        let config = parse_proxy_config(text);
        assert!(config.http.enabled);
        assert_eq!(config.http.host, Some("proxy.example.com".to_string()));
        assert_eq!(config.http.port, Some(8080));
    }

    #[test]
    fn pac_enabled_extracts_url() {
        let text = "<dictionary> {\n  \
                     ProxyAutoConfigEnable : 1\n  \
                     ProxyAutoConfigURLString : http://example.com/proxy.pac\n\
                     }\n";
        let config = parse_proxy_config(text);
        assert_eq!(
            config.pac_url,
            Some("http://example.com/proxy.pac".to_string())
        );
    }

    #[test]
    fn pac_url_ignored_when_disabled() {
        let text = "<dictionary> {\n  \
                     ProxyAutoConfigEnable : 0\n  \
                     ProxyAutoConfigURLString : http://example.com/proxy.pac\n\
                     }\n";
        let config = parse_proxy_config(text);
        assert_eq!(config.pac_url, None);
    }

    #[test]
    fn scoped_dictionary_does_not_clobber_top_level_values() {
        let text = "<dictionary> {\n  \
                     HTTPEnable : 0\n  \
                     __SCOPED__ : <dictionary> {\n    \
                     en0 : <dictionary> {\n      \
                     HTTPEnable : 1\n      \
                     HTTPProxy : scoped.example.com\n      \
                     HTTPPort : 3128\n    \
                     }\n  \
                     }\n\
                     }\n";
        let config = parse_proxy_config(text);
        assert!(!config.http.enabled);
        assert_eq!(config.http.host, None);
        assert_eq!(config.http.port, None);
    }

    #[test]
    fn exceptions_list_parsed() {
        let text = "<dictionary> {\n  \
                     ExceptionsList : <array> {\n    \
                     0 : *.local\n    \
                     1 : 169.254/16\n  \
                     }\n\
                     }\n";
        let config = parse_proxy_config(text);
        assert_eq!(
            config.exceptions,
            vec!["*.local".to_string(), "169.254/16".to_string()]
        );
    }
}
