use netdev::interface::state::OperState;
use netdev::interface::types::InterfaceType;
use netdev::ipnet;
use serde::Serialize;

/// Observable status of a single network interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Interface {
    pub name: String,
    pub up: bool,
    pub loopback: bool,
    pub addresses: Vec<String>,
}

pub(crate) struct RawInterface {
    pub name: String,
    pub oper_state: OperState,
    pub if_type: InterfaceType,
    pub ipv4: Vec<ipnet::Ipv4Net>,
    pub ipv6: Vec<ipnet::Ipv6Net>,
}

pub(crate) fn build_interface(raw: RawInterface) -> Interface {
    let mut addresses: Vec<String> = raw.ipv4.iter().map(ToString::to_string).collect();
    addresses.extend(raw.ipv6.iter().map(ToString::to_string));

    Interface {
        name: raw.name,
        up: raw.oper_state == OperState::Up,
        loopback: raw.if_type == InterfaceType::Loopback,
        addresses,
    }
}

pub(crate) fn is_link_local(address: &str) -> bool {
    let host = address.split('/').next().unwrap_or(address);
    let host = host.to_ascii_lowercase();
    host.starts_with("fe80:") || host.starts_with("169.254.")
}

/// Coarse operational classification of an interface, derived from its
/// up/down state and the routability of its addresses. Declared
/// online-to-offline so the derived `Ord` sorts interfaces that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum InterfaceClass {
    Routable,
    LinkLocalOnly,
    Unaddressed,
    Down,
}

pub fn classify_interface(iface: &Interface) -> InterfaceClass {
    if !iface.up {
        return InterfaceClass::Down;
    }
    if iface.addresses.is_empty() {
        return InterfaceClass::Unaddressed;
    }
    if iface.addresses.iter().all(|a| is_link_local(a)) {
        return InterfaceClass::LinkLocalOnly;
    }
    InterfaceClass::Routable
}

/// Classification of a single address, for per-address display.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum AddressClass {
    LinkLocal,
    RoutableV4,
    RoutableV6,
}

pub fn classify_address(address: &str) -> AddressClass {
    if is_link_local(address) {
        AddressClass::LinkLocal
    } else if address.contains(':') {
        AddressClass::RoutableV6
    } else {
        AddressClass::RoutableV4
    }
}

/// Returns the status of every network interface on the host.
pub fn list_interfaces() -> Vec<Interface> {
    netdev::get_interfaces()
        .into_iter()
        .map(|iface| {
            build_interface(RawInterface {
                name: iface.name,
                oper_state: iface.oper_state,
                if_type: iface.if_type,
                ipv4: iface.ipv4,
                ipv6: iface.ipv6,
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn test_iface(name: &str, up: bool, addresses: &[&str]) -> Interface {
    Interface {
        name: name.to_string(),
        up,
        loopback: false,
        addresses: addresses.iter().map(|s| s.to_string()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_interface_up_with_address() {
        let raw = RawInterface {
            name: "en0".to_string(),
            oper_state: OperState::Up,
            if_type: InterfaceType::Ethernet,
            ipv4: vec!["192.168.1.42/24".parse().unwrap()],
            ipv6: vec![],
        };

        let got = build_interface(raw);

        assert_eq!(
            got,
            Interface {
                name: "en0".to_string(),
                up: true,
                loopback: false,
                addresses: vec!["192.168.1.42/24".to_string()],
            }
        );
    }

    #[test]
    fn build_interface_down_with_no_address() {
        let raw = RawInterface {
            name: "en1".to_string(),
            oper_state: OperState::Down,
            if_type: InterfaceType::Ethernet,
            ipv4: vec![],
            ipv6: vec![],
        };

        let got = build_interface(raw);

        assert_eq!(
            got,
            Interface {
                name: "en1".to_string(),
                up: false,
                loopback: false,
                addresses: vec![],
            }
        );
    }

    #[test]
    fn is_link_local_detects_ipv4_self_assigned() {
        assert!(is_link_local("169.254.1.2/16"));
        assert!(!is_link_local("192.168.1.1/24"));
    }

    #[test]
    fn classify_down_interface() {
        let iface = Interface {
            name: "en1".to_string(),
            up: false,
            loopback: false,
            addresses: vec!["192.168.1.5/24".to_string()],
        };
        assert_eq!(classify_interface(&iface), InterfaceClass::Down);
    }

    #[test]
    fn classify_up_with_no_addresses() {
        let iface = Interface {
            name: "en1".to_string(),
            up: true,
            loopback: false,
            addresses: vec![],
        };
        assert_eq!(classify_interface(&iface), InterfaceClass::Unaddressed);
    }

    #[test]
    fn classify_up_with_only_link_local_addresses() {
        let iface = Interface {
            name: "en1".to_string(),
            up: true,
            loopback: false,
            addresses: vec!["fe80::1/64".to_string(), "169.254.1.2/16".to_string()],
        };
        assert_eq!(classify_interface(&iface), InterfaceClass::LinkLocalOnly);
    }

    #[test]
    fn classify_up_with_routable_address() {
        let iface = Interface {
            name: "en0".to_string(),
            up: true,
            loopback: false,
            addresses: vec!["fe80::1/64".to_string(), "192.168.1.5/24".to_string()],
        };
        assert_eq!(classify_interface(&iface), InterfaceClass::Routable);
    }

    #[test]
    fn interface_class_orders_online_to_offline() {
        assert!(InterfaceClass::Routable < InterfaceClass::LinkLocalOnly);
        assert!(InterfaceClass::LinkLocalOnly < InterfaceClass::Unaddressed);
        assert!(InterfaceClass::Unaddressed < InterfaceClass::Down);
    }

    #[test]
    fn classify_address_variants() {
        assert_eq!(classify_address("fe80::1/64"), AddressClass::LinkLocal);
        assert_eq!(classify_address("169.254.1.2/16"), AddressClass::LinkLocal);
        assert_eq!(classify_address("192.168.1.5/24"), AddressClass::RoutableV4);
        assert_eq!(classify_address("2001:db8::1/64"), AddressClass::RoutableV6);
    }

    #[test]
    fn build_interface_loopback() {
        let raw = RawInterface {
            name: "lo0".to_string(),
            oper_state: OperState::Up,
            if_type: InterfaceType::Loopback,
            ipv4: vec!["127.0.0.1/8".parse().unwrap()],
            ipv6: vec![],
        };

        let got = build_interface(raw);

        assert_eq!(
            got,
            Interface {
                name: "lo0".to_string(),
                up: true,
                loopback: true,
                addresses: vec!["127.0.0.1/8".to_string()],
            }
        );
    }
}
