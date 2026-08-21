use crate::connect::{self, ConnectResult};
use crate::dns::{self, Resolver};
use crate::interfaces::{self, Interface};
use crate::ip_stack::{self, IpStack};
use crate::proxy::{self, ProxyConfig};
use crate::reachability::{self, PingResult};
use crate::resolution::{self, DEFAULT_RESOLUTION_TARGETS, ResolutionResult};
use crate::vpn::{self, VpnStatus};
use crate::wifi::{self, WifiIdentity, WifiRadio, WifiStatus};
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
    WifiIdentity(WifiIdentity),
    WifiRadio(WifiRadio),
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
        // Two independent probes, not one: wifi_radio() is a fast CoreWLAN
        // call (no shell-out) while wifi_identity() shells to
        // system_profiler (~1s). Splitting them means the radio panel's
        // fields show up immediately instead of waiting on the slow SSID
        // lookup.
        scope.spawn(|| {
            let _ = tx.send(StatusField::WifiRadio(wifi::wifi_radio()));
        });
        scope.spawn(|| {
            let _ = tx.send(StatusField::WifiIdentity(wifi::wifi_identity()));
        });
    });
}

/// Accumulates `StatusField`s as they stream in from `collect_streaming`,
/// one `Option` field per `StatusField` variant. Shared by every UI
/// (`netcheck-tui`, `netcheck-gui`) so a new `StatusField` variant only
/// needs its `merge`/`has_any` arm added once instead of once per UI.
#[derive(Debug, Clone, Default)]
pub struct PartialStatus {
    pub interfaces: Option<Vec<Interface>>,
    pub vpn: Option<VpnStatus>,
    pub split_dns: Option<bool>,
    pub resolvers: Option<Vec<Resolver>>,
    pub reachability: Option<Vec<PingResult>>,
    pub reachability_v6: Option<Vec<PingResult>>,
    pub resolution: Option<Vec<ResolutionResult>>,
    pub domain_reachability: Option<Vec<ConnectResult>>,
    pub proxy: Option<ProxyConfig>,
    pub wifi_identity: Option<WifiIdentity>,
    pub wifi_radio: Option<WifiRadio>,
    pub ip_stack: Option<IpStack>,
}

impl PartialStatus {
    pub fn merge(&mut self, field: StatusField) {
        match field {
            StatusField::Interfaces(v) => self.interfaces = Some(v),
            StatusField::Vpn(v) => self.vpn = Some(v),
            StatusField::Resolvers(v) => self.resolvers = Some(v),
            StatusField::SplitDns(v) => self.split_dns = Some(v),
            StatusField::Reachability(v) => self.reachability = Some(v),
            StatusField::ReachabilityV6(v) => self.reachability_v6 = Some(v),
            StatusField::Resolution(v) => self.resolution = Some(v),
            StatusField::DomainReachability(v) => self.domain_reachability = Some(v),
            StatusField::Proxy(v) => self.proxy = Some(v),
            StatusField::WifiIdentity(v) => self.wifi_identity = Some(v),
            StatusField::WifiRadio(v) => self.wifi_radio = Some(v),
            StatusField::IpStack(v) => self.ip_stack = Some(v),
        }
    }

    pub fn has_any(&self) -> bool {
        self.interfaces.is_some()
            || self.vpn.is_some()
            || self.resolvers.is_some()
            || self.split_dns.is_some()
            || self.reachability.is_some()
            || self.reachability_v6.is_some()
            || self.resolution.is_some()
            || self.domain_reachability.is_some()
            || self.proxy.is_some()
            || self.wifi_identity.is_some()
            || self.wifi_radio.is_some()
            || self.ip_stack.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::mem::discriminant;

    /// Guards the send side of `collect_streaming`: every `StatusField`
    /// variant must be sent exactly once. The receive side (each UI's
    /// `PartialStatus::merge()`) is protected by an exhaustive match with no
    /// wildcard arm, but nothing stops someone from adding a 12th variant
    /// and forgetting the corresponding `scope.spawn(...)` block — that
    /// would compile and pass every other test while silently wedging a UI
    /// panel on "Collecting..." forever. This shells out to real macOS
    /// tools (ping, scutil, system_profiler); it asserts on message count
    /// and variant coverage, not on reachability values, so it's
    /// deterministic even without live network.
    #[test]
    fn collect_streaming_sends_every_status_field_variant_exactly_once() {
        let (tx, rx) = mpsc::channel();
        collect_streaming(tx);

        let received: Vec<StatusField> = rx.into_iter().collect();
        assert_eq!(
            received.len(),
            12,
            "expected exactly 12 StatusField messages, got {}: {received:?}",
            received.len()
        );

        let all_variants: [StatusField; 12] = [
            StatusField::Interfaces(Vec::new()),
            StatusField::Vpn(VpnStatus {
                tunnels: Vec::new(),
                primary_interface: None,
                connected: false,
                split_tunnel: false,
                routed_subnets: Vec::new(),
            }),
            StatusField::Resolvers(Vec::new()),
            StatusField::SplitDns(false),
            StatusField::Reachability(Vec::new()),
            StatusField::ReachabilityV6(Vec::new()),
            StatusField::Resolution(Vec::new()),
            StatusField::DomainReachability(Vec::new()),
            StatusField::Proxy(proxy::ProxyConfig {
                http: proxy::ProxyEndpoint {
                    enabled: false,
                    host: None,
                    port: None,
                },
                https: proxy::ProxyEndpoint {
                    enabled: false,
                    host: None,
                    port: None,
                },
                socks: proxy::ProxyEndpoint {
                    enabled: false,
                    host: None,
                    port: None,
                },
                pac_url: None,
                exceptions: Vec::new(),
            }),
            StatusField::WifiIdentity(WifiIdentity::default()),
            StatusField::WifiRadio(WifiRadio::default()),
            StatusField::IpStack(IpStack::None),
        ];

        let mut seen: HashSet<usize> = HashSet::new();
        for field in &received {
            let matched = all_variants
                .iter()
                .position(|variant| discriminant(variant) == discriminant(field))
                .unwrap_or_else(|| panic!("unexpected StatusField variant: {field:?}"));
            assert!(
                seen.insert(matched),
                "StatusField variant sent more than once: {field:?}"
            );
        }
        assert_eq!(
            seen.len(),
            all_variants.len(),
            "not every StatusField variant was sent"
        );
    }
}
