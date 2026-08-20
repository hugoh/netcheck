use crate::connect::{self, ConnectResult};
use crate::dns::{self, Resolver};
use crate::interfaces::{self, Interface};
use crate::reachability::{self, PingResult};
use crate::resolution::{self, DEFAULT_RESOLUTION_TARGETS, ResolutionResult};
use crate::vpn::{self, VpnStatus};
use serde::Serialize;

/// Well-known public IPs used for reachability probes: Cloudflare, Google,
/// Quad9, and OpenDNS anycast resolvers.
pub const DEFAULT_PING_TARGETS: &[&str] = &[
    "1.1.1.1",
    "1.0.0.1",
    "8.8.8.8",
    "8.8.4.4",
    "9.9.9.9",
    "208.67.222.222",
    "208.67.220.220",
];

/// A full snapshot of the machine's network status.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkStatus {
    pub interfaces: Vec<Interface>,
    pub vpn: VpnStatus,
    pub resolvers: Vec<Resolver>,
    pub split_dns: bool,
    pub reachability: Vec<PingResult>,
    pub resolution: Vec<ResolutionResult>,
    /// TCP connect (port 443) reachability for the well-known domain list.
    /// Uses a real connect rather than ICMP ping because several large
    /// operators (Amazon, Microsoft) filter ICMP at their edge regardless
    /// of whether the service itself is up.
    pub domain_reachability: Vec<ConnectResult>,
}

/// Collects a full network status snapshot. Shells out to `scutil` and
/// `ping`, and resolves DNS, so this is not free — expect it to take on the
/// order of a second.
pub fn collect() -> NetworkStatus {
    let interfaces = interfaces::list_interfaces();
    let vpn = vpn::vpn_status(&interfaces);
    let resolvers = dns::list_resolvers();
    let split_dns = dns::has_split_dns(&resolvers);
    let reachability = reachability::ping_all(DEFAULT_PING_TARGETS);
    let resolution = resolution::resolve_all(DEFAULT_RESOLUTION_TARGETS);
    let domain_reachability = connect::connect_all(DEFAULT_RESOLUTION_TARGETS, 443);

    NetworkStatus {
        interfaces,
        vpn,
        resolvers,
        split_dns,
        reachability,
        resolution,
        domain_reachability,
    }
}
