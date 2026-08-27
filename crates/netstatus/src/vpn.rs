use crate::interfaces::{Interface, is_link_local};
use crate::sc_store::{cf_dict_array, cf_string, cf_string_array, get_dict};
use serde::Serialize;
use system_configuration::dynamic_store::{SCDynamicStore, SCDynamicStoreBuilder};

/// VPN / tunnel status derived from the interface list and the primary
/// (default-route) interface.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct VpnStatus {
    pub tunnels: Vec<String>,
    pub primary_interface: Option<String>,
    pub connected: bool,
    pub split_tunnel: bool,
    /// Subnets routed through the VPN (its own assigned address/subnet,
    /// plus any additional split-tunnel destinations it pushes), formatted
    /// as CIDR strings. Empty when not connected, or when the dynamic
    /// store's per-service IPv4 dict for the tunnel interface carries no
    /// `AdditionalRoutes` — a full-tunnel VPN commonly has none, since it
    /// becomes the default route (`OverridePrimary`) instead of pushing a
    /// discrete route list.
    pub routed_subnets: Vec<String>,
}

fn is_tunnel_interface(name: &str) -> bool {
    name.starts_with("utun") || name.starts_with("ppp") || name.starts_with("tun")
}

fn has_routable_address(interface: &Interface) -> bool {
    interface.addresses.iter().any(|a| !is_link_local(a))
}

pub(crate) fn classify_tunnels(
    interfaces: &[Interface],
    primary: Option<&str>,
    routed_subnets: Vec<String>,
) -> VpnStatus {
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
        routed_subnets: if connected {
            routed_subnets
        } else {
            Vec::new()
        },
    }
}

fn mask_to_prefix_len(mask: &str) -> Option<u8> {
    let addr: std::net::Ipv4Addr = mask.parse().ok()?;
    Some(u32::from(addr).count_ones() as u8)
}

fn cidr(addr: &str, mask: &str) -> String {
    match mask_to_prefix_len(mask) {
        Some(prefix) => format!("{addr}/{prefix}"),
        None => addr.to_string(),
    }
}

/// Pairs `Addresses` with `SubnetMasks` by position. The dynamic store
/// normally keeps these arrays the same length, but if it ever reports a
/// mismatch (a mid-reconfiguration read, or a malformed VPN-pushed state),
/// every address is still reported — as a bare address if no mask lines up
/// with it — rather than silently dropping the trailing entries with
/// `Iterator::zip`.
fn own_subnet_routes(addresses: &[String], subnet_masks: &[String]) -> Vec<String> {
    addresses
        .iter()
        .enumerate()
        .map(|(i, addr)| match subnet_masks.get(i) {
            Some(mask) => cidr(addr, mask),
            None => addr.to_string(),
        })
        .collect()
}

/// Reads the routed subnets for `if_name`'s network service: its own
/// assigned address/subnet (`Addresses`/`SubnetMasks`) plus any discrete
/// split-tunnel destinations it pushes (`AdditionalRoutes`, each
/// `{DestinationAddress, SubnetMask}`).
fn routes_for_interface(store: &SCDynamicStore, if_name: &str) -> Vec<String> {
    let Some(keys) = store.get_keys("State:/Network/Service/.*/IPv4") else {
        return Vec::new();
    };

    for key in keys.iter() {
        let key_str = key.to_string();
        let Some(dict) = get_dict(store, &key_str) else {
            continue;
        };
        if cf_string(&dict, "InterfaceName").as_deref() != Some(if_name) {
            continue;
        }

        let addresses = cf_string_array(&dict, "Addresses");
        let subnet_masks = cf_string_array(&dict, "SubnetMasks");
        let mut routes = own_subnet_routes(&addresses, &subnet_masks);

        for route in cf_dict_array(&dict, "AdditionalRoutes") {
            if let Some(dest) = cf_string(&route, "DestinationAddress") {
                let mask = cf_string(&route, "SubnetMask").unwrap_or_default();
                routes.push(cidr(&dest, &mask));
            }
        }

        return routes;
    }

    Vec::new()
}

/// Combines the dynamic store's primary-interface state with the interface
/// list to determine VPN/tunnel status.
pub fn vpn_status(interfaces: &[Interface]) -> VpnStatus {
    let Some(store) = SCDynamicStoreBuilder::new("netcheck-vpn").build() else {
        return classify_tunnels(interfaces, None, Vec::new());
    };
    let primary = get_dict(&store, "State:/Network/Global/IPv4")
        .and_then(|dict| cf_string(&dict, "PrimaryInterface"));

    let routed_subnets = interfaces
        .iter()
        .filter(|i| i.up && is_tunnel_interface(&i.name) && has_routable_address(i))
        .flat_map(|i| routes_for_interface(&store, &i.name))
        .collect();

    classify_tunnels(interfaces, primary.as_deref(), routed_subnets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interfaces::test_iface as iface;

    #[test]
    fn no_vpn_when_utuns_have_no_address() {
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun0", true, &[]),
        ];
        let status = classify_tunnels(&interfaces, Some("en0"), Vec::new());
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
        let status = classify_tunnels(&interfaces, Some("en0"), Vec::new());
        assert!(!status.connected);
        assert!(status.tunnels.is_empty());
    }

    #[test]
    fn split_tunnel_when_vpn_tunnel_present_but_primary_is_physical() {
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun3", true, &["10.10.0.5/24"]),
        ];
        let status = classify_tunnels(&interfaces, Some("en0"), Vec::new());
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
        let status = classify_tunnels(&interfaces, Some("utun3"), Vec::new());
        assert!(status.connected);
        assert!(!status.split_tunnel);
    }

    #[test]
    fn routed_subnets_pass_through_when_connected() {
        let interfaces = vec![
            iface("en0", true, &["192.168.1.42/24"]),
            iface("utun3", true, &["10.10.0.5/24"]),
        ];
        let status = classify_tunnels(&interfaces, Some("en0"), vec!["10.223.36.41/8".to_string()]);
        assert_eq!(status.routed_subnets, vec!["10.223.36.41/8".to_string()]);
    }

    #[test]
    fn routed_subnets_cleared_when_not_connected() {
        let interfaces = vec![iface("en0", true, &["192.168.1.42/24"])];
        let status = classify_tunnels(&interfaces, Some("en0"), vec!["10.223.36.41/8".to_string()]);
        assert!(status.routed_subnets.is_empty());
    }

    #[test]
    fn mask_to_prefix_len_converts_common_masks() {
        assert_eq!(mask_to_prefix_len("255.0.0.0"), Some(8));
        assert_eq!(mask_to_prefix_len("255.255.255.0"), Some(24));
        assert_eq!(mask_to_prefix_len("255.255.255.255"), Some(32));
        assert_eq!(mask_to_prefix_len("not-an-ip"), None);
    }

    #[test]
    fn cidr_formats_address_and_mask() {
        assert_eq!(cidr("10.223.36.41", "255.0.0.0"), "10.223.36.41/8");
        assert_eq!(cidr("192.168.68.67", "255.255.255.255"), "192.168.68.67/32");
    }

    #[test]
    fn own_subnet_routes_pairs_addresses_with_masks() {
        let addresses = vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()];
        let masks = vec!["255.0.0.0".to_string(), "255.255.255.0".to_string()];
        assert_eq!(
            own_subnet_routes(&addresses, &masks),
            vec!["10.0.0.1/8".to_string(), "10.0.0.2/24".to_string()]
        );
    }

    #[test]
    fn own_subnet_routes_keeps_trailing_address_when_masks_run_short() {
        let addresses = vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()];
        let masks = vec!["255.0.0.0".to_string()];
        assert_eq!(
            own_subnet_routes(&addresses, &masks),
            vec!["10.0.0.1/8".to_string(), "10.0.0.2".to_string()]
        );
    }
}
