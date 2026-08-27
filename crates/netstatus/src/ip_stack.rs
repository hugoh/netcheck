use crate::interfaces::{Interface, is_link_local};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, schemars::JsonSchema)]
pub enum IpStack {
    Ipv4Only,
    Ipv6Only,
    DualStack,
    None,
}

pub fn detect_ip_stack(interfaces: &[Interface]) -> IpStack {
    let mut has_v4 = false;
    let mut has_v6 = false;

    for iface in interfaces.iter().filter(|i| i.up && !i.loopback) {
        for addr in &iface.addresses {
            if is_link_local(addr) {
                continue;
            }
            if addr.contains(':') {
                has_v6 = true;
            } else {
                has_v4 = true;
            }
        }
    }

    match (has_v4, has_v6) {
        (true, true) => IpStack::DualStack,
        (true, false) => IpStack::Ipv4Only,
        (false, true) => IpStack::Ipv6Only,
        (false, false) => IpStack::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interfaces::test_iface as iface;

    #[test]
    fn dual_stack_when_both_families_present() {
        let interfaces = vec![iface("en0", true, &["192.168.1.42/24", "2001:db8::1/64"])];
        assert_eq!(detect_ip_stack(&interfaces), IpStack::DualStack);
    }

    #[test]
    fn ipv4_only_when_no_routable_v6() {
        let interfaces = vec![iface("en0", true, &["192.168.1.42/24", "fe80::1/64"])];
        assert_eq!(detect_ip_stack(&interfaces), IpStack::Ipv4Only);
    }

    #[test]
    fn ipv6_only_when_no_v4() {
        let interfaces = vec![iface("en0", true, &["2001:db8::1/64"])];
        assert_eq!(detect_ip_stack(&interfaces), IpStack::Ipv6Only);
    }

    #[test]
    fn none_when_no_routable_address() {
        let interfaces = vec![iface("en0", true, &["fe80::1/64"])];
        assert_eq!(detect_ip_stack(&interfaces), IpStack::None);
    }

    #[test]
    fn ignores_down_and_loopback_interfaces() {
        let mut lo = iface("lo0", true, &["127.0.0.1/8", "::1/128"]);
        lo.loopback = true;
        let down = iface("en1", false, &["10.0.0.5/24"]);
        let interfaces = vec![lo, down];
        assert_eq!(detect_ip_stack(&interfaces), IpStack::None);
    }
}
