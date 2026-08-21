use crate::connect::{self, ConnectResult};
use crate::dns::{self, Resolver};
use crate::interfaces::{self, Interface};
use crate::ip_stack::{self, IpStack};
use crate::proxy::{self, ProxyConfig};
use crate::reachability::{self, PingResult};
use crate::resolution::{self, DEFAULT_RESOLUTION_TARGETS, ResolutionResult};
use crate::vpn::{self, VpnStatus};
use crate::wifi::{self, WifiStatus};
use serde::Serialize;
use std::sync::mpsc;

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

/// Well-known public IPv6 anycast targets: Cloudflare, Google, Quad9.
pub const DEFAULT_PING_TARGETS_V6: &[&str] = &[
    "2606:4700:4700::1111",
    "2606:4700:4700::1001",
    "2001:4860:4860::8888",
    "2001:4860:4860::8844",
    "2620:fe::fe",
];

/// A full snapshot of the machine's network status.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkStatus {
    pub interfaces: Vec<Interface>,
    pub vpn: VpnStatus,
    pub resolvers: Vec<Resolver>,
    pub split_dns: bool,
    pub reachability: Vec<PingResult>,
    pub reachability_v6: Vec<PingResult>,
    pub resolution: Vec<ResolutionResult>,
    /// TCP connect (port 443) reachability for the well-known domain list.
    /// Uses a real connect rather than ICMP ping because several large
    /// operators (Amazon, Microsoft) filter ICMP at their edge regardless
    /// of whether the service itself is up.
    pub domain_reachability: Vec<ConnectResult>,
    pub proxy: ProxyConfig,
    pub wifi: WifiStatus,
    pub ip_stack: IpStack,
}

/// Collects a full network status snapshot. Shells out to `scutil`,
/// `ping`, and `system_profiler`, and resolves DNS; independent probes run
/// concurrently, so this takes roughly as long as the slowest single probe
/// (typically the Wi-Fi lookup, ~1s) rather than their sum.
pub fn collect() -> NetworkStatus {
    std::thread::scope(|scope| {
        let interfaces_handle = scope.spawn(interfaces::list_interfaces);
        let resolvers_handle = scope.spawn(dns::list_resolvers);
        let reachability_handle = scope.spawn(|| reachability::ping_all(DEFAULT_PING_TARGETS));
        let reachability_v6_handle =
            scope.spawn(|| reachability::ping_all(DEFAULT_PING_TARGETS_V6));
        let resolution_handle = scope.spawn(|| resolution::resolve_all(DEFAULT_RESOLUTION_TARGETS));
        let domain_reachability_handle =
            scope.spawn(|| connect::connect_all(DEFAULT_RESOLUTION_TARGETS, 443));
        let proxy_handle = scope.spawn(proxy::proxy_config);
        let wifi_handle = scope.spawn(wifi::wifi_status);

        let interfaces = interfaces_handle.join().unwrap();
        let vpn = vpn::vpn_status(&interfaces);
        let ip_stack = ip_stack::detect_ip_stack(&interfaces);
        let resolvers = resolvers_handle.join().unwrap();
        let split_dns = dns::has_split_dns(&resolvers);

        NetworkStatus {
            reachability: reachability_handle.join().unwrap(),
            reachability_v6: reachability_v6_handle.join().unwrap(),
            resolution: resolution_handle.join().unwrap(),
            domain_reachability: domain_reachability_handle.join().unwrap(),
            proxy: proxy_handle.join().unwrap(),
            wifi: wifi_handle.join().unwrap(),
            interfaces,
            vpn,
            resolvers,
            split_dns,
            ip_stack,
        }
    })
}

/// One probe's result, delivered as soon as that probe completes. Exactly
/// one variant per `NetworkStatus` field.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum StatusField {
    Interfaces(Vec<Interface>),
    Vpn(VpnStatus),
    Resolvers(Vec<Resolver>),
    SplitDns(bool),
    Reachability(Vec<PingResult>),
    ReachabilityV6(Vec<PingResult>),
    Resolution(Vec<ResolutionResult>),
    DomainReachability(Vec<ConnectResult>),
    Proxy(ProxyConfig),
    Wifi(WifiStatus),
    IpStack(IpStack),
}

/// Runs every probe concurrently and sends each `StatusField` down `tx` the
/// moment it completes, instead of waiting for the slowest probe before any
/// result is visible. Send errors (receiver dropped) are ignored — the
/// caller is expected to stop reading, not this function to stop probing.
pub fn collect_streaming(tx: mpsc::Sender<StatusField>) {
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let interfaces = interfaces::list_interfaces();
            let _ = tx.send(StatusField::IpStack(ip_stack::detect_ip_stack(&interfaces)));
            let _ = tx.send(StatusField::Vpn(vpn::vpn_status(&interfaces)));
            let _ = tx.send(StatusField::Interfaces(interfaces));
        });
        scope.spawn(|| {
            let resolvers = dns::list_resolvers();
            let _ = tx.send(StatusField::SplitDns(dns::has_split_dns(&resolvers)));
            let _ = tx.send(StatusField::Resolvers(resolvers));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::Reachability(reachability::ping_all(
                DEFAULT_PING_TARGETS,
            )));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::ReachabilityV6(reachability::ping_all(
                DEFAULT_PING_TARGETS_V6,
            )));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::Resolution(resolution::resolve_all(
                DEFAULT_RESOLUTION_TARGETS,
            )));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::DomainReachability(connect::connect_all(
                DEFAULT_RESOLUTION_TARGETS,
                443,
            )));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::Proxy(proxy::proxy_config()));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::Wifi(wifi::wifi_status()));
        });
    });
}
