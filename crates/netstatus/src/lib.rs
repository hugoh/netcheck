mod connect;
mod dns;
mod interfaces;
mod ip_stack;
mod proxy;
mod reachability;
mod resolution;
mod status;
mod vpn;
mod wifi;

pub use connect::{ConnectResult, connect, connect_all};
pub use dns::{Resolver, has_split_dns, list_resolvers};
pub use interfaces::{
    AddressClass, Interface, InterfaceClass, classify_address, classify_interface,
    list_interfaces,
};
pub use ip_stack::{IpStack, detect_ip_stack};
pub use proxy::{ProxyConfig, ProxyEndpoint, proxy_config};
pub use reachability::{PingResult, ping, ping_all};
pub use resolution::{DEFAULT_RESOLUTION_TARGETS, ResolutionResult, resolve, resolve_all};
pub use status::{
    DEFAULT_PING_TARGETS, DEFAULT_PING_TARGETS_V6, NetworkStatus, StatusField, collect,
    collect_streaming,
};
pub use vpn::{VpnStatus, vpn_status};
pub use wifi::{WifiStatus, wifi_status};
