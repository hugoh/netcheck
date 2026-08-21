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
    host.to_ascii_lowercase().starts_with("fe80:")
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
