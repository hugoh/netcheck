mod captive_portal;
mod concurrent;
mod connect;
mod dns;
mod gateway;
mod interfaces;
mod ip_stack;
mod proxy;
mod reachability;
mod resolution;
mod sc_store;
mod sc_watch;
mod status;
mod vpn;
mod wifi;

/// The running binary's version — a release tag (e.g. "1.2.3") set by CI via
/// `NETCHECK_VERSION`, or `dev-<short-sha>` (or plain `dev` with no git) for
/// local builds. See `build.rs`.
pub const VERSION: &str = env!("NETCHECK_VERSION");

pub use captive_portal::{CaptivePortalStatus, check_captive_portal};
pub use connect::{ConnectResult, connect, connect_all};
pub use dns::{Resolver, has_split_dns, list_resolvers, nameserver_ips, vpn_scoped_domains};
pub use gateway::default_gateway_ips;
pub use interfaces::{
    AddressClass, Interface, InterfaceClass, classify_address, classify_interface, list_interfaces,
};
pub use ip_stack::{IpStack, detect_ip_stack};
pub use proxy::{ProxyConfig, ProxyEndpoint, proxy_config};
pub use reachability::{PingResult, ping, ping_all};
pub use resolution::{DEFAULT_RESOLUTION_TARGETS, ResolutionResult, resolve, resolve_all};
pub use sc_watch::{WatchHandle, watch_config_changes};
pub use status::{
    ConnectionConfidence, DEFAULT_PING_TARGETS, DEFAULT_PING_TARGETS_V6, NetworkStatus,
    PartialStatus, StatusField, WatchCommand, collect, collect_confidence_streaming,
    collect_streaming, confidence_only, status_field_schema, watch_command_schema,
};
pub use vpn::{VpnStatus, vpn_status};
pub use wifi::{WifiIdentity, WifiRadio, WifiStatus, wifi_identity, wifi_radio, wifi_status};
