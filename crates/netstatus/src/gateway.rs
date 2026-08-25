use netdev::NetworkDevice;

/// Builds the gateway's routable IP addresses (v4 addresses first, then
/// v6) from a `NetworkDevice` record — split out from `default_gateway_ips`
/// so it's testable without depending on the live default route.
pub(crate) fn build_gateway_ips(device: &NetworkDevice) -> Vec<String> {
    let mut ips: Vec<String> = device.ipv4.iter().map(ToString::to_string).collect();
    ips.extend(device.ipv6.iter().map(ToString::to_string));
    ips
}

/// Returns the default gateway's IP addresses, or an empty list if the
/// default route (or its gateway) can't be determined.
pub fn default_gateway_ips() -> Vec<String> {
    netdev::get_default_gateway()
        .map(|device| build_gateway_ips(&device))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use netdev::MacAddr;

    #[test]
    fn build_gateway_ips_orders_v4_before_v6() {
        let device = NetworkDevice {
            mac_addr: MacAddr::zero(),
            ipv4: vec!["192.168.1.1".parse().unwrap()],
            ipv6: vec!["fe80::1".parse().unwrap()],
        };
        assert_eq!(
            build_gateway_ips(&device),
            vec!["192.168.1.1".to_string(), "fe80::1".to_string()]
        );
    }

    #[test]
    fn build_gateway_ips_empty_when_device_has_no_addresses() {
        let device = NetworkDevice::new();
        assert!(build_gateway_ips(&device).is_empty());
    }
}
