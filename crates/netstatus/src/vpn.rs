use crate::interfaces::{Interface, is_link_local};
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use serde::Serialize;
use system_configuration::dynamic_store::SCDynamicStoreBuilder;

/// VPN / tunnel status derived from the interface list and the primary
/// (default-route) interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VpnStatus {
    pub tunnels: Vec<String>,
    pub primary_interface: Option<String>,
    pub connected: bool,
    pub split_tunnel: bool,
}

fn is_tunnel_interface(name: &str) -> bool {
    name.starts_with("utun") || name.starts_with("ppp") || name.starts_with("tun")
}

fn has_routable_address(interface: &Interface) -> bool {
    interface.addresses.iter().any(|a| !is_link_local(a))
}

/// Reads the primary (default-route) interface name from the System
/// Configuration dynamic store's global IPv4 state, the same data
/// `scutil --nwi` derives and prints as its "Network interfaces:" trailer.
fn primary_interface_from_store() -> Option<String> {
    let store = SCDynamicStoreBuilder::new("netcheck-vpn").build()?;
    let value = store.get("State:/Network/Global/IPv4")?;
    let opaque: CFDictionary = value.downcast_into()?;
    let dict: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_get_rule(opaque.as_concrete_TypeRef()) };
    dict.find(CFString::from("PrimaryInterface"))
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string())
}

pub(crate) fn classify_tunnels(interfaces: &[Interface], primary: Option<&str>) -> VpnStatus {
    let tunnels: Vec<String> = interfaces
        .iter()
        .filter(|i| i.up && is_tunnel_interface(&i.name) && has_routable_address(i))
        .map(|i| i.name.clone())
        .collect();

    let connected = !tunnels.is_empty();
    let split_tunnel = connected && primary.is_some_and(|p| !is_tunnel_interface(p));

    VpnStatus {
        tunnels,
        primary_interface: primary.map(str::to_string),
        connected,
        split_tunnel,
    }
}

/// Combines the dynamic store's primary-interface state with the interface
/// list to determine VPN/tunnel status.
pub fn vpn_status(interfaces: &[Interface]) -> VpnStatus {
    let primary = primary_interface_from_store();
    classify_tunnels(interfaces, primary.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, up: bool, addresses: &[&str]) -> Interface {
        Interface {
            name: name.to_string(),
            up,
            loopback: false,
            addresses: addresses.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn no_vpn_when_utuns_have_no_address() {
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun0", true, &[]),
        ];
        let status = classify_tunnels(&interfaces, Some("en0"));
        assert!(!status.connected);
        assert!(status.tunnels.is_empty());
    }

    #[test]
    fn no_vpn_when_only_link_local_utuns_present() {
        // Baseline macOS noise: several utun interfaces are up with only a
        // link-local IPv6 address even when no VPN is connected.
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun0", true, &["fe80::e404:92fb:4376:3062/64"]),
            iface("utun1", true, &["fe80::665d:ab22:4c96:324e/64"]),
        ];
        let status = classify_tunnels(&interfaces, Some("en0"));
        assert!(!status.connected);
        assert!(status.tunnels.is_empty());
    }

    #[test]
    fn split_tunnel_when_vpn_tunnel_present_but_primary_is_physical() {
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun3", true, &["10.10.0.5/24"]),
        ];
        let status = classify_tunnels(&interfaces, Some("en0"));
        assert!(status.connected);
        assert!(status.split_tunnel);
        assert_eq!(status.tunnels, vec!["utun3".to_string()]);
    }

    #[test]
    fn full_tunnel_when_primary_route_is_the_vpn_interface() {
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun3", true, &["10.10.0.5/24"]),
        ];
        let status = classify_tunnels(&interfaces, Some("utun3"));
        assert!(status.connected);
        assert!(!status.split_tunnel);
    }
}
