use crate::captive_portal::{self, CaptivePortalStatus};
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
    "1.0.0.1",
    "1.1.1.1",
    "8.8.4.4",
    "8.8.8.8",
    "9.9.9.9",
    "208.67.220.220",
    "208.67.222.222",
];

/// Well-known public IPv6 anycast targets: Cloudflare, Google, Quad9.
pub const DEFAULT_PING_TARGETS_V6: &[&str] = &[
    "2001:4860:4860::8844",
    "2001:4860:4860::8888",
    "2606:4700:4700::1001",
    "2606:4700:4700::1111",
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
    pub captive_portal: CaptivePortalStatus,
}

/// How confident netcheck is that the machine has a working internet
/// connection, derived from three independent signal categories rather
/// than any single probe (a host that blocks ICMP but serves DNS and TCP
/// fine shouldn't read as offline). Capped at `Limited` when a captive
/// portal is confirmed present, regardless of how the three categories
/// vote — a portal means the user isn't actually on the internet even if
/// DNS/ping/TCP all reach the portal's own infrastructure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ConnectionConfidence {
    Online,
    Limited,
    Offline,
}

/// True if more than half of `results` satisfy `is_ok`. A single lucky
/// reply from an otherwise-unreachable target list shouldn't count as a
/// category "working" — this requires the majority of probed targets to
/// agree. Empty lists are never ok.
fn majority_ok<T>(results: &[T], is_ok: impl Fn(&T) -> bool) -> bool {
    let n = results.len();
    n > 0 && results.iter().filter(|r| is_ok(r)).count() * 2 > n
}

/// Rolls up whether DNS resolution, ICMP reachability, and TCP connect
/// each had a majority of their probed targets succeed into a single
/// confidence tier. 3-of-3 or 2-of-3 categories succeeding means the
/// connection is up even if one probe type is filtered somewhere on the
/// path; 1-of-3 is a degraded connection; 0-of-3 is offline.
fn confidence_from(dns_ok: bool, ping_ok: bool, tcp_ok: bool) -> ConnectionConfidence {
    match [dns_ok, ping_ok, tcp_ok].iter().filter(|ok| **ok).count() {
        3 | 2 => ConnectionConfidence::Online,
        1 => ConnectionConfidence::Limited,
        _ => ConnectionConfidence::Offline,
    }
}

/// Caps `confidence` at `Limited` when `captive_portal` confirms a portal
/// is present. `Unknown` (the probe didn't get a conclusive answer) is not
/// evidence of a portal, so it doesn't trigger the cap.
fn cap_for_captive_portal(
    confidence: ConnectionConfidence,
    captive_portal: CaptivePortalStatus,
) -> ConnectionConfidence {
    if captive_portal == CaptivePortalStatus::Detected && confidence == ConnectionConfidence::Online
    {
        ConnectionConfidence::Limited
    } else {
        confidence
    }
}

impl NetworkStatus {
    pub fn confidence(&self) -> ConnectionConfidence {
        let dns_ok = majority_ok(&self.resolution, |r| r.resolved);
        let ping_ok = majority_ok(&self.reachability, |p| p.reachable)
            || majority_ok(&self.reachability_v6, |p| p.reachable);
        let tcp_ok = majority_ok(&self.domain_reachability, |c| c.reachable);
        cap_for_captive_portal(
            confidence_from(dns_ok, ping_ok, tcp_ok),
            self.captive_portal,
        )
    }
}

/// The four probes that determine `ConnectionConfidence`, run once and
/// shared by both `collect()` (which needs the raw results in
/// `NetworkStatus`) and `confidence_only()` (which needs only the derived
/// tier) instead of each spawning its own copy of the same probes.
struct CoreSignals {
    resolution: Vec<ResolutionResult>,
    reachability: Vec<PingResult>,
    reachability_v6: Vec<PingResult>,
    domain_reachability: Vec<ConnectResult>,
}

impl CoreSignals {
    fn collect() -> Self {
        std::thread::scope(|scope| {
            let resolution_handle =
                scope.spawn(|| resolution::resolve_all(DEFAULT_RESOLUTION_TARGETS));
            let reachability_handle = scope.spawn(|| reachability::ping_all(DEFAULT_PING_TARGETS));
            let reachability_v6_handle =
                scope.spawn(|| reachability::ping_all(DEFAULT_PING_TARGETS_V6));
            let domain_reachability_handle =
                scope.spawn(|| connect::connect_all(DEFAULT_RESOLUTION_TARGETS, 443));

            CoreSignals {
                resolution: resolution_handle.join().unwrap(),
                reachability: reachability_handle.join().unwrap(),
                reachability_v6: reachability_v6_handle.join().unwrap(),
                domain_reachability: domain_reachability_handle.join().unwrap(),
            }
        })
    }

    fn confidence(&self) -> ConnectionConfidence {
        let dns_ok = majority_ok(&self.resolution, |r| r.resolved);
        let ping_ok = majority_ok(&self.reachability, |p| p.reachable)
            || majority_ok(&self.reachability_v6, |p| p.reachable);
        let tcp_ok = majority_ok(&self.domain_reachability, |c| c.reachable);
        confidence_from(dns_ok, ping_ok, tcp_ok)
    }
}

/// Derives connection confidence from only the three signal categories it
/// needs — DNS resolution, ICMP reachability (v4+v6), TCP connect —
/// skipping everything else `collect()` runs (Wi-Fi, proxy, interfaces,
/// VPN, and the captive-portal probe). This is a known accuracy tradeoff
/// for speed: unlike `NetworkStatus::confidence()`, this path can't apply
/// the captive-portal cap, since it never probes for one. Use this instead
/// of `collect().confidence()` when only the confidence tier is needed.
pub fn confidence_only() -> ConnectionConfidence {
    CoreSignals::collect().confidence()
}

/// Collects a full network status snapshot. Shells out to `scutil`,
/// `ping`, and `system_profiler`, resolves DNS, and probes for a captive
/// portal; independent probes run concurrently, so this takes roughly as
/// long as the slowest single probe rather than their sum. Most probes
/// (e.g. the Wi-Fi lookup) typically finish within ~1s, but the
/// captive-portal probe has a 10s timeout, making it the worst-case bound
/// on a slow or unresponsive network.
pub fn collect() -> NetworkStatus {
    std::thread::scope(|scope| {
        let interfaces_handle = scope.spawn(interfaces::list_interfaces);
        let resolvers_handle = scope.spawn(dns::list_resolvers);
        let core_signals_handle = scope.spawn(CoreSignals::collect);
        let proxy_handle = scope.spawn(proxy::proxy_config);
        let wifi_handle = scope.spawn(wifi::wifi_status);
        let captive_portal_handle = scope.spawn(captive_portal::check_captive_portal);

        let interfaces = interfaces_handle.join().unwrap();
        let vpn = vpn::vpn_status(&interfaces);
        let ip_stack = ip_stack::detect_ip_stack(&interfaces);
        let resolvers = resolvers_handle.join().unwrap();
        let split_dns = dns::has_split_dns(&resolvers);
        let core_signals = core_signals_handle.join().unwrap();

        NetworkStatus {
            reachability: core_signals.reachability,
            reachability_v6: core_signals.reachability_v6,
            resolution: core_signals.resolution,
            domain_reachability: core_signals.domain_reachability,
            proxy: proxy_handle.join().unwrap(),
            wifi: wifi_handle.join().unwrap(),
            captive_portal: captive_portal_handle.join().unwrap(),
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
    CaptivePortal(CaptivePortalStatus),
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
        scope.spawn(|| {
            let _ = tx.send(StatusField::CaptivePortal(
                captive_portal::check_captive_portal(),
            ));
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
    pub captive_portal: Option<CaptivePortalStatus>,
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
            StatusField::CaptivePortal(v) => self.captive_portal = Some(v),
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
            || self.captive_portal.is_some()
    }

    /// `None` until resolution, both reachability probes, domain
    /// reachability, and the captive-portal probe have all arrived — a
    /// partial view of only some signal categories would misreport
    /// confidence (e.g. reading `Limited` just because DNS hasn't come back
    /// yet, not because it failed, or reading `Online` before the
    /// captive-portal cap has had a chance to apply).
    pub fn confidence(&self) -> Option<ConnectionConfidence> {
        let resolution = self.resolution.as_ref()?;
        let reachability = self.reachability.as_ref()?;
        let reachability_v6 = self.reachability_v6.as_ref()?;
        let domain_reachability = self.domain_reachability.as_ref()?;
        let captive_portal = self.captive_portal.as_ref()?;
        let dns_ok = majority_ok(resolution, |r| r.resolved);
        let ping_ok = majority_ok(reachability, |p| p.reachable)
            || majority_ok(reachability_v6, |p| p.reachable);
        let tcp_ok = majority_ok(domain_reachability, |c| c.reachable);
        Some(cap_for_captive_portal(
            confidence_from(dns_ok, ping_ok, tcp_ok),
            *captive_portal,
        ))
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
            13,
            "expected exactly 13 StatusField messages, got {}: {received:?}",
            received.len()
        );

        let all_variants: [StatusField; 13] = [
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
            StatusField::CaptivePortal(crate::captive_portal::CaptivePortalStatus::Unknown),
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

    #[test]
    fn confidence_only_runs_without_the_full_snapshot() {
        // Smoke test: confidence_only() must compile, run to completion, and
        // return a valid ConnectionConfidence without requiring NetworkStatus
        // or PartialStatus — the whole point is it's independent of collect().
        let result = confidence_only();
        assert!(matches!(
            result,
            ConnectionConfidence::Online
                | ConnectionConfidence::Limited
                | ConnectionConfidence::Offline
        ));
    }

    #[test]
    fn confidence_from_all_signals_ok_is_online() {
        assert_eq!(
            confidence_from(true, true, true),
            ConnectionConfidence::Online
        );
    }

    #[test]
    fn confidence_from_two_signals_ok_is_online() {
        assert_eq!(
            confidence_from(true, true, false),
            ConnectionConfidence::Online
        );
        assert_eq!(
            confidence_from(true, false, true),
            ConnectionConfidence::Online
        );
        assert_eq!(
            confidence_from(false, true, true),
            ConnectionConfidence::Online
        );
    }

    #[test]
    fn confidence_from_one_signal_ok_is_limited() {
        assert_eq!(
            confidence_from(true, false, false),
            ConnectionConfidence::Limited
        );
        assert_eq!(
            confidence_from(false, true, false),
            ConnectionConfidence::Limited
        );
        assert_eq!(
            confidence_from(false, false, true),
            ConnectionConfidence::Limited
        );
    }

    #[test]
    fn confidence_from_no_signals_ok_is_offline() {
        assert_eq!(
            confidence_from(false, false, false),
            ConnectionConfidence::Offline
        );
    }

    fn resolution_results(oks: &[bool]) -> Vec<ResolutionResult> {
        oks.iter()
            .map(|&resolved| ResolutionResult {
                domain: "example.com".to_string(),
                resolved,
                addresses: Vec::new(),
                duration_ms: None,
            })
            .collect()
    }

    fn ping_results(oks: &[bool]) -> Vec<PingResult> {
        oks.iter()
            .map(|&reachable| PingResult {
                target: "1.1.1.1".to_string(),
                reachable,
                rtt_ms: None,
            })
            .collect()
    }

    fn connect_results(oks: &[bool]) -> Vec<ConnectResult> {
        oks.iter()
            .map(|&reachable| ConnectResult {
                target: "example.com".to_string(),
                port: 443,
                reachable,
                rtt_ms: None,
            })
            .collect()
    }

    #[test]
    fn majority_ok_requires_more_than_half() {
        assert!(
            !majority_ok(
                &ping_results(&[true, false, false, false, false, false, false]),
                |p| p.reachable
            ),
            "1 of 7 must not count as ok"
        );
        assert!(
            !majority_ok(
                &ping_results(&[true, true, true, false, false, false, false]),
                |p| p.reachable
            ),
            "3 of 7 must not count as ok"
        );
        assert!(
            majority_ok(
                &ping_results(&[true, true, true, true, false, false, false]),
                |p| p.reachable
            ),
            "4 of 7 must count as ok"
        );
        assert!(
            !majority_ok::<PingResult>(&[], |p| p.reachable),
            "empty list must never count as ok"
        );
    }

    #[test]
    fn network_status_confidence_needs_majority_per_category() {
        let mut status = NetworkStatus {
            interfaces: Vec::new(),
            vpn: vpn::VpnStatus {
                tunnels: Vec::new(),
                primary_interface: None,
                connected: false,
                split_tunnel: false,
                routed_subnets: Vec::new(),
            },
            resolvers: Vec::new(),
            split_dns: false,
            reachability: ping_results(&[true, false, false, false, false, false, false]),
            reachability_v6: ping_results(&[false, false, false, false, false]),
            resolution: resolution_results(&[true, false, false, false, false, false, false]),
            domain_reachability: connect_results(&[true, false, false, false, false, false, false]),
            proxy: proxy::ProxyConfig {
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
            },
            wifi: WifiStatus::default(),
            ip_stack: IpStack::None,
            captive_portal: captive_portal::CaptivePortalStatus::Clear,
        };
        // Only 1-of-7 succeeding in every category used to read Online under
        // `.any()`; under majority it must read Offline.
        assert_eq!(status.confidence(), ConnectionConfidence::Offline);

        // Bring ping to a v6 majority (3 of 5) while v4 and the rest stay
        // minority — ping_ok should come from the v6 stack alone (OR of
        // per-stack majorities, not a combined pool).
        status.reachability_v6 = ping_results(&[true, true, true, false, false]);
        assert_eq!(status.confidence(), ConnectionConfidence::Limited);
    }

    #[test]
    fn captive_portal_detected_caps_online_at_limited() {
        assert_eq!(
            cap_for_captive_portal(
                ConnectionConfidence::Online,
                captive_portal::CaptivePortalStatus::Detected
            ),
            ConnectionConfidence::Limited
        );
    }

    #[test]
    fn captive_portal_unknown_does_not_cap() {
        assert_eq!(
            cap_for_captive_portal(
                ConnectionConfidence::Online,
                captive_portal::CaptivePortalStatus::Unknown
            ),
            ConnectionConfidence::Online
        );
    }

    #[test]
    fn captive_portal_clear_does_not_cap() {
        assert_eq!(
            cap_for_captive_portal(
                ConnectionConfidence::Online,
                captive_portal::CaptivePortalStatus::Clear
            ),
            ConnectionConfidence::Online
        );
    }

    #[test]
    fn captive_portal_cap_does_not_raise_offline_or_limited() {
        assert_eq!(
            cap_for_captive_portal(
                ConnectionConfidence::Limited,
                captive_portal::CaptivePortalStatus::Detected
            ),
            ConnectionConfidence::Limited
        );
        assert_eq!(
            cap_for_captive_portal(
                ConnectionConfidence::Offline,
                captive_portal::CaptivePortalStatus::Detected
            ),
            ConnectionConfidence::Offline
        );
    }

    #[test]
    fn partial_status_confidence_waits_for_captive_portal() {
        let mut partial = PartialStatus {
            resolution: Some(resolution_results(&[true; 7])),
            reachability: Some(ping_results(&[true; 7])),
            reachability_v6: Some(ping_results(&[true; 5])),
            domain_reachability: Some(connect_results(&[true; 7])),
            ..Default::default()
        };
        assert_eq!(partial.confidence(), None);

        partial.captive_portal = Some(captive_portal::CaptivePortalStatus::Detected);
        assert_eq!(partial.confidence(), Some(ConnectionConfidence::Limited));
    }
}
