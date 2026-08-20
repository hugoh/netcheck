use crate::interfaces::Interface;
use serde::Serialize;
use std::process::Command;

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

fn is_link_local(address: &str) -> bool {
    let host = address.split('/').next().unwrap_or(address);
    host.to_ascii_lowercase().starts_with("fe80:")
}

fn has_routable_address(interface: &Interface) -> bool {
    interface.addresses.iter().any(|a| !is_link_local(a))
}

/// Parses the "Network interfaces: en0 utun3" trailer line from `scutil --nwi`
/// and returns the first (primary) interface name.
pub(crate) fn parse_primary_interface(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("Network interfaces:"))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
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

/// Runs `scutil --nwi` and combines it with the interface list to determine
/// VPN/tunnel status.
pub fn vpn_status(interfaces: &[Interface]) -> VpnStatus {
    let output = Command::new("scutil")
        .arg("--nwi")
        .output()
        .expect("scutil --nwi should be runnable on macOS");
    let text = String::from_utf8_lossy(&output.stdout);
    let primary = parse_primary_interface(&text);
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
    fn parse_primary_interface_from_nwi_trailer() {
        let text = "Network information\n...\nNetwork interfaces: en0 utun3\n";
        assert_eq!(parse_primary_interface(text), Some("en0".to_string()));
    }

    #[test]
    fn parse_primary_interface_missing_returns_none() {
        assert_eq!(parse_primary_interface("Network information\n"), None);
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
