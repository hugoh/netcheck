use crate::sc_store::{Dict, cf_string, cf_string_array, get_dict};
use serde::Serialize;
use std::ffi::CString;
use system_configuration::dynamic_store::{SCDynamicStore, SCDynamicStoreBuilder};
use system_configuration::network_reachability::{ReachabilityFlags, SCNetworkReachability};

/// A single resolver configuration, one per network service.
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

/// Builds a `Resolver` from a service's DNS dictionary
/// (`State:/Network/Service/<id>/DNS`) plus the interface/scope/reachability
/// data gathered separately (this function stays pure and testable; the
/// dynamic-store and reachability lookups that produce its arguments live
/// in `list_resolvers`).
pub(crate) fn build_resolver(
    dns_dict: &Dict,
    if_name: Option<String>,
    if_index: Option<u32>,
    scoped: bool,
    reachable: bool,
) -> Resolver {
    Resolver {
        domain: cf_string(dns_dict, "DomainName"),
        search_domains: cf_string_array(dns_dict, "SearchDomains"),
        nameservers: cf_string_array(dns_dict, "ServerAddresses"),
        if_index,
        if_name,
        scoped,
        reachable,
    }
}

/// Returns true if any non-scoped resolver's search/query domain applies
/// only through a specific interface (i.e. split-DNS via a VPN tunnel).
pub fn has_split_dns(resolvers: &[Resolver]) -> bool {
    resolvers
        .iter()
        .any(|r| r.scoped && r.if_name.as_deref().is_some_and(|n| n.starts_with("utun")))
}

/// Domains only resolvable via a VPN tunnel's own resolver — i.e. every
/// scoped resolver's domain (falling back to its first search domain) for
/// resolvers bound to a `utun*` interface. Deduplicated, order preserved.
pub fn vpn_scoped_domains(resolvers: &[Resolver]) -> Vec<String> {
    let mut domains = Vec::new();
    for r in resolvers {
        if !r.scoped || !r.if_name.as_deref().is_some_and(|n| n.starts_with("utun")) {
            continue;
        }
        if let Some(domain) = r
            .domain
            .clone()
            .or_else(|| r.search_domains.first().cloned())
            && !domains.contains(&domain)
        {
            domains.push(domain);
        }
    }
    domains
}

fn interface_name_for_service(store: &SCDynamicStore, dns_key: &str) -> Option<String> {
    let ipv4_key = dns_key.replace("/DNS", "/IPv4");
    if let Some(dict) = get_dict(store, &ipv4_key)
        && let Some(name) = cf_string(&dict, "InterfaceName")
    {
        return Some(name);
    }
    let ipv6_key = dns_key.replace("/DNS", "/IPv6");
    get_dict(store, &ipv6_key).and_then(|dict| cf_string(&dict, "InterfaceName"))
}

fn interface_index(name: &str) -> Option<u32> {
    let c_name = CString::new(name).ok()?;
    let idx = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
    (idx != 0).then_some(idx)
}

fn is_reachable(nameservers: &[String]) -> bool {
    let Some(first) = nameservers.first() else {
        return false;
    };
    let Ok(c_addr) = CString::new(first.as_str()) else {
        return false;
    };
    SCNetworkReachability::from_host(&c_addr)
        .and_then(|r| r.reachability().ok())
        .is_some_and(|flags| flags.contains(ReachabilityFlags::REACHABLE))
}

/// Every network service's DNS configuration is scoped to that service's own
/// interface (mirroring `scutil --dns`'s "for scoped queries" section); the
/// service currently backing the default route (`PrimaryService`) is also
/// emitted a second time as the unscoped, systemwide-default resolver.
///
/// This is a from-scratch derivation from `SCDynamicStore`, not a byte-exact
/// reproduction of `scutil --dns`'s merged/ordered/mDNS-aware output — it
/// omits secondary system resolvers (e.g. the `.local` mDNS entry) that
/// `dns_configuration_copy()` (a private SPI `scutil` uses internally, with
/// no public equivalent) injects. For netcheck's purpose — resolver
/// visibility and split-DNS detection — this is sufficient.
pub fn list_resolvers() -> Vec<Resolver> {
    let Some(store) = SCDynamicStoreBuilder::new("netcheck-dns").build() else {
        return Vec::new();
    };

    let primary_service = get_dict(&store, "State:/Network/Global/IPv4")
        .and_then(|dict| cf_string(&dict, "PrimaryService"));

    let Some(keys) = store.get_keys("State:/Network/Service/.*/DNS") else {
        return Vec::new();
    };

    let mut resolvers = Vec::new();
    for key in keys.iter() {
        let key_str = key.to_string();
        let Some(dns_dict) = get_dict(&store, &key_str) else {
            continue;
        };
        let nameservers = cf_string_array(&dns_dict, "ServerAddresses");
        let if_name = interface_name_for_service(&store, &key_str);
        let if_index = if_name.as_deref().and_then(interface_index);
        let reachable = is_reachable(&nameservers);

        let is_primary = primary_service
            .as_deref()
            .is_some_and(|p| key_str.contains(&format!("/Service/{p}/")));

        resolvers.push(build_resolver(
            &dns_dict,
            if_name.clone(),
            if_index,
            true,
            reachable,
        ));

        if is_primary {
            resolvers.push(build_resolver(&dns_dict, None, None, false, reachable));
        }
    }

    resolvers
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    fn dns_dict(domain: Option<&str>, search: &[&str], servers: &[&str]) -> Dict {
        let mut pairs: Vec<(CFString, CFType)> = Vec::new();
        if let Some(d) = domain {
            pairs.push((CFString::from("DomainName"), CFString::from(d).as_CFType()));
        }
        let search_arr = CFArray::from_CFTypes(
            &search
                .iter()
                .map(|s| CFString::from(*s))
                .collect::<Vec<_>>(),
        );
        pairs.push((CFString::from("SearchDomains"), search_arr.as_CFType()));
        let servers_arr = CFArray::from_CFTypes(
            &servers
                .iter()
                .map(|s| CFString::from(*s))
                .collect::<Vec<_>>(),
        );
        pairs.push((CFString::from("ServerAddresses"), servers_arr.as_CFType()));
        CFDictionary::from_CFType_pairs(&pairs)
    }

    #[test]
    fn builds_scoped_resolver_from_dns_dict() {
        let dict = dns_dict(Some("corp.example.com"), &["lan"], &["10.10.10.10"]);
        let r = build_resolver(&dict, Some("utun3".to_string()), Some(20), true, true);

        assert_eq!(r.domain, Some("corp.example.com".to_string()));
        assert_eq!(r.search_domains, vec!["lan".to_string()]);
        assert_eq!(r.nameservers, vec!["10.10.10.10".to_string()]);
        assert_eq!(r.if_name, Some("utun3".to_string()));
        assert_eq!(r.if_index, Some(20));
        assert!(r.scoped);
        assert!(r.reachable);
    }

    #[test]
    fn builds_unscoped_resolver_with_no_interface() {
        let dict = dns_dict(None, &[], &["9.9.9.9"]);
        let r = build_resolver(&dict, None, None, false, true);

        assert!(!r.scoped);
        assert_eq!(r.if_name, None);
        assert_eq!(r.if_index, None);
    }

    fn resolver(
        domain: Option<&str>,
        search_domains: &[&str],
        nameservers: &[&str],
        if_index: Option<u32>,
        if_name: Option<&str>,
    ) -> Resolver {
        Resolver {
            domain: domain.map(String::from),
            search_domains: search_domains.iter().map(|s| s.to_string()).collect(),
            nameservers: nameservers.iter().map(|s| s.to_string()).collect(),
            if_index,
            if_name: if_name.map(String::from),
            scoped: true,
            reachable: true,
        }
    }

    fn en0_resolver() -> Resolver {
        resolver(None, &[], &["9.9.9.9"], Some(11), Some("en0"))
    }

    fn utun3_resolver() -> Resolver {
        resolver(
            Some("corp.example.com"),
            &[],
            &["10.10.10.10"],
            Some(20),
            Some("utun3"),
        )
    }

    #[test]
    fn detects_split_dns_via_vpn_tunnel() {
        let resolvers = vec![en0_resolver(), utun3_resolver()];
        assert!(has_split_dns(&resolvers));
    }

    #[test]
    fn no_split_dns_when_no_scoped_tunnel_resolver() {
        assert!(!has_split_dns(&[en0_resolver()]));
    }

    #[test]
    fn vpn_scoped_domains_collects_tunnel_resolver_domains() {
        let utun4_resolver = resolver(
            None,
            &["vpn.example.net"],
            &["192.0.2.31"],
            Some(19),
            Some("utun4"),
        );
        let resolvers = vec![en0_resolver(), utun3_resolver(), utun4_resolver];
        assert_eq!(
            vpn_scoped_domains(&resolvers),
            vec![
                "corp.example.com".to_string(),
                "vpn.example.net".to_string()
            ]
        );
    }

    #[test]
    fn vpn_scoped_domains_empty_without_tunnel_resolver() {
        let lan_resolver = resolver(Some("lan"), &[], &["9.9.9.9"], Some(11), Some("en0"));
        assert!(vpn_scoped_domains(&[lan_resolver]).is_empty());
    }
}
