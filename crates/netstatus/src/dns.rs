use serde::Serialize;
use std::process::Command;

/// A single resolver block from `scutil --dns`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolver {
    pub domain: Option<String>,
    pub search_domains: Vec<String>,
    pub nameservers: Vec<String>,
    pub if_index: Option<u32>,
    pub if_name: Option<String>,
    pub scoped: bool,
    pub reachable: bool,
}

pub(crate) fn parse_scutil_dns(text: &str) -> Vec<Resolver> {
    let mut resolvers = Vec::new();
    let mut scoped = false;
    let mut current: Option<Resolver> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("DNS configuration (for scoped queries)") {
            scoped = true;
            continue;
        }

        if trimmed.starts_with("resolver #") {
            if let Some(r) = current.take() {
                resolvers.push(r);
            }
            current = Some(Resolver {
                domain: None,
                search_domains: Vec::new(),
                nameservers: Vec::new(),
                if_index: None,
                if_name: None,
                scoped,
                reachable: false,
            });
            continue;
        }

        let Some(resolver) = current.as_mut() else {
            continue;
        };

        if let Some(value) = field_value(trimmed, "domain") {
            resolver.domain = Some(value.to_string());
        } else if trimmed.starts_with("search domain[") {
            if let Some(value) = trimmed.split(':').nth(1) {
                resolver.search_domains.push(value.trim().to_string());
            }
        } else if trimmed.starts_with("nameserver[") {
            if let Some(value) = trimmed.split(':').nth(1) {
                resolver.nameservers.push(value.trim().to_string());
            }
        } else if let Some(value) = field_value(trimmed, "if_index") {
            // Format: "11 (en0)"
            let mut parts = value.splitn(2, ' ');
            if let Some(idx) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
                resolver.if_index = Some(idx);
            }
            if let Some(name) = parts.next() {
                resolver.if_name = Some(name.trim_matches(|c| c == '(' || c == ')').to_string());
            }
        } else if let Some(value) = field_value(trimmed, "reach") {
            resolver.reachable = value.contains("Reachable") && !value.contains("Not Reachable");
        }
    }

    if let Some(r) = current.take() {
        resolvers.push(r);
    }

    resolvers
}

fn field_value<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let (name, value) = line.split_once(':')?;
    if name.trim() == field {
        Some(value.trim())
    } else {
        None
    }
}

/// Returns true if any non-scoped resolver's search/query domain applies
/// only through a specific interface (i.e. split-DNS via a VPN tunnel).
pub fn has_split_dns(resolvers: &[Resolver]) -> bool {
    resolvers
        .iter()
        .any(|r| r.scoped && r.if_name.as_deref().is_some_and(|n| n.starts_with("utun")))
}

/// Runs `scutil --dns` and parses the result.
pub fn list_resolvers() -> Vec<Resolver> {
    let output = Command::new("scutil")
        .arg("--dns")
        .output()
        .expect("scutil --dns should be runnable on macOS");
    let text = String::from_utf8_lossy(&output.stdout);
    parse_scutil_dns(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
DNS configuration

resolver #1
  search domain[0] : lan
  nameserver[0] : 9.9.9.9
  flags    : Request A records, Request AAAA records
  reach    : 0x00000002 (Reachable)

resolver #2
  domain   : local
  options  : mdns
  timeout  : 5
  flags    : Request A records, Request AAAA records
  reach    : 0x00000000 (Not Reachable)
  order    : 300000

DNS configuration (for scoped queries)

resolver #1
  search domain[0] : lan
  nameserver[0] : 9.9.9.9
  if_index : 11 (en0)
  flags    : Scoped, Request A records, Request AAAA records
  reach    : 0x00000002 (Reachable)

resolver #2
  domain   : corp.example.com
  nameserver[0] : 10.10.10.10
  if_index : 20 (utun3)
  flags    : Scoped, Request A records, Request AAAA records
  reach    : 0x00000002 (Reachable)
"#;

    #[test]
    fn parses_unscoped_and_scoped_resolvers() {
        let resolvers = parse_scutil_dns(SAMPLE);
        assert_eq!(resolvers.len(), 4);

        assert_eq!(resolvers[0].search_domains, vec!["lan".to_string()]);
        assert_eq!(resolvers[0].nameservers, vec!["9.9.9.9".to_string()]);
        assert!(!resolvers[0].scoped);
        assert!(resolvers[0].reachable);

        assert_eq!(resolvers[1].domain, Some("local".to_string()));
        assert!(!resolvers[1].reachable);
    }

    #[test]
    fn parses_scoped_resolver_with_if_index() {
        let resolvers = parse_scutil_dns(SAMPLE);
        let vpn_resolver = &resolvers[3];

        assert!(vpn_resolver.scoped);
        assert_eq!(vpn_resolver.domain, Some("corp.example.com".to_string()));
        assert_eq!(vpn_resolver.if_index, Some(20));
        assert_eq!(vpn_resolver.if_name, Some("utun3".to_string()));
    }

    #[test]
    fn detects_split_dns_via_vpn_tunnel() {
        let resolvers = parse_scutil_dns(SAMPLE);
        assert!(has_split_dns(&resolvers));
    }

    #[test]
    fn no_split_dns_when_no_scoped_tunnel_resolver() {
        let resolvers: Vec<Resolver> = parse_scutil_dns(SAMPLE)
            .into_iter()
            .filter(|r| r.if_name.as_deref() != Some("utun3"))
            .collect();
        assert!(!has_split_dns(&resolvers));
    }
}
