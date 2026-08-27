use crate::captive_portal::{self, CaptivePortalStatus};
use crate::connect::{self, ConnectResult};
use crate::dns::{self, Resolver};
use crate::gateway;
use crate::interfaces::{self, Interface};
use crate::ip_stack::{self, IpStack};
use crate::proxy::{self, ProxyConfig};
use crate::reachability::{self, PingResult};
use crate::resolution::{self, DEFAULT_RESOLUTION_TARGETS, ResolutionResult};
use crate::vpn::{self, VpnStatus};
use crate::wifi::{self, WifiIdentity, WifiRadio, WifiStatus};
use serde::{Deserialize, Serialize};
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
    /// ICMP reachability of the default gateway's IP(s) — a LAN-hop signal,
    /// distinct from `reachability`'s public anycast targets: unreachable
    /// here alongside unreachable public targets points at the local link
    /// rather than the ISP or upstream.
    pub gateway_reachability: Vec<PingResult>,
    /// ICMP reachability of each configured resolver's IP, probed directly
    /// rather than inferred from whether name resolution succeeds.
    pub nameserver_reachability: Vec<PingResult>,
    /// TCP connect (port 53) reachability of each configured resolver's
    /// IP. Like `domain_reachability`, this catches resolvers that filter
    /// ICMP but still serve queries.
    pub nameserver_connect: Vec<ConnectResult>,
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
        let gateway_reachability_handle = scope.spawn(|| {
            let ips = gateway::default_gateway_ips();
            let targets: Vec<&str> = ips.iter().map(String::as_str).collect();
            reachability::ping_all(&targets)
        });

        let interfaces = interfaces_handle.join().unwrap();
        let vpn = vpn::vpn_status(&interfaces);
        let ip_stack = ip_stack::detect_ip_stack(&interfaces);
        let resolvers = resolvers_handle.join().unwrap();
        let split_dns = dns::has_split_dns(&resolvers);
        let nameserver_ips = dns::nameserver_ips(&resolvers);
        let ping_targets = nameserver_ips.clone();
        let connect_targets = nameserver_ips.clone();
        let nameserver_reachability_handle = scope.spawn(move || {
            let targets: Vec<&str> = ping_targets.iter().map(String::as_str).collect();
            reachability::ping_all(&targets)
        });
        let nameserver_connect_handle = scope.spawn(move || {
            let targets: Vec<&str> = connect_targets.iter().map(String::as_str).collect();
            connect::connect_all(&targets, 53)
        });
        let core_signals = core_signals_handle.join().unwrap();

        NetworkStatus {
            reachability: core_signals.reachability,
            reachability_v6: core_signals.reachability_v6,
            resolution: core_signals.resolution,
            domain_reachability: core_signals.domain_reachability,
            gateway_reachability: gateway_reachability_handle.join().unwrap(),
            nameserver_reachability: nameserver_reachability_handle.join().unwrap(),
            nameserver_connect: nameserver_connect_handle.join().unwrap(),
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

/// Which target list a streamed `Ping`/`Connect` item, or a `GroupComplete`
/// marker, belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
pub enum ProbeGroup {
    /// ICMP reachability of well-known public IPv4 anycast DNS resolvers
    /// (see `DEFAULT_PING_TARGETS`).
    #[schemars(title = "Reachability")]
    Reachability,
    /// Same as `Reachability`, but IPv6 targets (see
    /// `DEFAULT_PING_TARGETS_V6`).
    #[schemars(title = "ReachabilityV6")]
    ReachabilityV6,
    /// ICMP reachability of the default gateway's IP(s) — a LAN-hop
    /// signal, distinct from `Reachability`'s public anycast targets:
    /// unreachable here alongside unreachable public targets points at the
    /// local link rather than the ISP or upstream.
    #[schemars(title = "GatewayReachability")]
    GatewayReachability,
    /// ICMP reachability of each configured resolver's IP, probed directly
    /// rather than inferred from whether name resolution succeeds.
    #[schemars(title = "NameserverReachability")]
    NameserverReachability,
    /// TCP connect (port 443) reachability of well-known domains (see
    /// `DEFAULT_RESOLUTION_TARGETS`) — a real connect rather than ICMP
    /// ping, since some large operators filter ICMP at their edge
    /// regardless of whether the service itself is up.
    #[schemars(title = "DomainReachability")]
    DomainReachability,
    /// TCP connect (port 53) reachability of each configured resolver's
    /// IP. Like `DomainReachability`, this catches resolvers that filter
    /// ICMP but still serve queries.
    #[schemars(title = "NameserverConnect")]
    NameserverConnect,
    /// DNS resolution of well-known domains (see
    /// `DEFAULT_RESOLUTION_TARGETS`).
    #[schemars(title = "Resolution")]
    Resolution,
}

/// A command the `watch` subcommand accepts as NDJSON on stdin (one per
/// line), symmetric with the `StatusField` NDJSON it writes to stdout.
/// `watch` running at all already means auto-refresh is enabled — there's
/// no separate enable/disable command, just "trigger a check now" for a
/// process the caller already knows is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
pub enum WatchCommand {
    /// Run a full check immediately, same as a config-change fire.
    Refresh,
}

/// One probe's result, delivered as soon as that probe completes. For a
/// target-list probe (`Ping`/`Connect`/`Resolution`), that's per-target, not
/// per-group — a slow or unreachable target no longer holds up the rest of
/// its list from being visible (see `for_each_concurrent`). `GroupComplete`
/// marks a target list as fully drained; only sent for the four groups
/// `PartialStatus::confidence` needs a completeness signal for.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub enum StatusField {
    /// The current network interfaces.
    #[schemars(title = "Interfaces")]
    Interfaces(Vec<Interface>),
    /// VPN/tunnel status derived from the interface list and the primary
    /// (default-route) interface.
    #[schemars(title = "Vpn")]
    Vpn(VpnStatus),
    /// Every DNS resolver macOS knows about, including scoped
    /// (per-interface) resolvers.
    #[schemars(title = "Resolvers")]
    Resolvers(Vec<Resolver>),
    /// Whether any resolver is scoped to a specific interface — the
    /// mechanism VPN clients like Cisco AnyConnect use for split-DNS.
    #[schemars(title = "SplitDns")]
    SplitDns(bool),
    /// One target's ICMP ping result, for the given `group`.
    #[schemars(title = "Ping")]
    Ping {
        group: ProbeGroup,
        result: PingResult,
    },
    /// One target's TCP connect result, for the given `group`.
    #[schemars(title = "Connect")]
    Connect {
        group: ProbeGroup,
        result: ConnectResult,
    },
    /// One domain's DNS resolution result (always `ProbeGroup::Resolution`
    /// — the only resolution group — so unlike `Ping`/`Connect` this isn't
    /// tagged with one).
    #[schemars(title = "Resolution")]
    Resolution(ResolutionResult),
    /// `group`'s target list has fully drained — every target in it has
    /// sent its `Ping`/`Connect`/`Resolution` result at least once.
    #[schemars(title = "GroupComplete")]
    GroupComplete(ProbeGroup),
    /// The system's configured HTTP/HTTPS/SOCKS/PAC proxy settings.
    #[schemars(title = "Proxy")]
    Proxy(ProxyConfig),
    /// SSID/connected-state — the slow half of Wi-Fi status
    /// (`system_profiler`-backed).
    #[schemars(title = "WifiIdentity")]
    WifiIdentity(WifiIdentity),
    /// Channel/signal/noise/security/PHY-mode — the fast half of Wi-Fi
    /// status (CoreWLAN-backed, no shell-out).
    #[schemars(title = "WifiRadio")]
    WifiRadio(WifiRadio),
    /// Whether the machine has a routable IPv4 address, IPv6 address,
    /// both, or neither.
    #[schemars(title = "IpStack")]
    IpStack(IpStack),
    /// Whether a captive portal was detected.
    #[schemars(title = "CaptivePortal")]
    CaptivePortal(CaptivePortalStatus),
}

/// JSON Schema for `StatusField` — the wire format streamed by the `stream`
/// and `watch` CLI subcommands (one object per NDJSON line). Generated
/// directly from the enum via `schemars` rather than hand-written, so it
/// can't silently drift from the actual Rust type the way independently
/// maintained docs (or the Swift decode types) can.
pub fn status_field_schema() -> schemars::Schema {
    schemars::schema_for!(StatusField)
}

/// Spawns the four probes that determine `ConnectionConfidence` — DNS
/// resolution, ICMP reachability (v4+v6), TCP connect — shared by
/// `collect_streaming` (which runs them alongside everything else) and
/// `collect_confidence_streaming` (which runs only these). Each target's
/// result streams individually as it completes, followed by one
/// `GroupComplete` once its whole list has drained.
fn spawn_core_signal_probes<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    tx: &mpsc::Sender<StatusField>,
) {
    {
        let tx = tx.clone();
        scope.spawn(move || {
            reachability::ping_each(DEFAULT_PING_TARGETS, |result| {
                let _ = tx.send(StatusField::Ping {
                    group: ProbeGroup::Reachability,
                    result,
                });
            });
            let _ = tx.send(StatusField::GroupComplete(ProbeGroup::Reachability));
        });
    }
    {
        let tx = tx.clone();
        scope.spawn(move || {
            reachability::ping_each(DEFAULT_PING_TARGETS_V6, |result| {
                let _ = tx.send(StatusField::Ping {
                    group: ProbeGroup::ReachabilityV6,
                    result,
                });
            });
            let _ = tx.send(StatusField::GroupComplete(ProbeGroup::ReachabilityV6));
        });
    }
    {
        let tx = tx.clone();
        scope.spawn(move || {
            resolution::resolve_each(DEFAULT_RESOLUTION_TARGETS, |result| {
                let _ = tx.send(StatusField::Resolution(result));
            });
            let _ = tx.send(StatusField::GroupComplete(ProbeGroup::Resolution));
        });
    }
    {
        let tx = tx.clone();
        scope.spawn(move || {
            connect::connect_each(DEFAULT_RESOLUTION_TARGETS, 443, |result| {
                let _ = tx.send(StatusField::Connect {
                    group: ProbeGroup::DomainReachability,
                    result,
                });
            });
            let _ = tx.send(StatusField::GroupComplete(ProbeGroup::DomainReachability));
        });
    }
}

/// Runs every probe concurrently and sends each `StatusField` down `tx` the
/// moment it completes, instead of waiting for the slowest probe before any
/// result is visible. Send errors (receiver dropped) are ignored — the
/// caller is expected to stop reading, not this function to stop probing.
pub fn collect_streaming(tx: mpsc::Sender<StatusField>) {
    std::thread::scope(|scope| {
        spawn_core_signal_probes(scope, &tx);
        scope.spawn(|| {
            let interfaces = interfaces::list_interfaces();
            let _ = tx.send(StatusField::IpStack(ip_stack::detect_ip_stack(&interfaces)));
            let _ = tx.send(StatusField::Vpn(vpn::vpn_status(&interfaces)));
            let _ = tx.send(StatusField::Interfaces(interfaces));
        });
        scope.spawn(|| {
            let resolvers = dns::list_resolvers();
            let _ = tx.send(StatusField::SplitDns(dns::has_split_dns(&resolvers)));
            let nameserver_ips = dns::nameserver_ips(&resolvers);
            let _ = tx.send(StatusField::Resolvers(resolvers));
            let targets: Vec<&str> = nameserver_ips.iter().map(String::as_str).collect();
            reachability::ping_each(&targets, |result| {
                let _ = tx.send(StatusField::Ping {
                    group: ProbeGroup::NameserverReachability,
                    result,
                });
            });
            connect::connect_each(&targets, 53, |result| {
                let _ = tx.send(StatusField::Connect {
                    group: ProbeGroup::NameserverConnect,
                    result,
                });
            });
        });
        scope.spawn(|| {
            let ips = gateway::default_gateway_ips();
            let targets: Vec<&str> = ips.iter().map(String::as_str).collect();
            reachability::ping_each(&targets, |result| {
                let _ = tx.send(StatusField::Ping {
                    group: ProbeGroup::GatewayReachability,
                    result,
                });
            });
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

/// Runs only the four confidence-determining probes (see
/// [`spawn_core_signal_probes`]), skipping everything that can't have
/// changed without an OS network-config event — interfaces, VPN, proxy,
/// Wi-Fi (notably the slow `system_profiler`-backed identity lookup),
/// ip-stack, and the captive-portal probe. Intended for a periodic
/// safety-net recheck (e.g. a blackholed route, which produces no config
/// event) where re-running the static-config probes would be pure waste.
pub fn collect_confidence_streaming(tx: mpsc::Sender<StatusField>) {
    std::thread::scope(|scope| {
        spawn_core_signal_probes(scope, &tx);
    });
}

/// Replaces the entry in `list` matching `item` by `key`, or appends it if
/// no entry matches — keeps a target's position stable across re-runs
/// (first-seen order) instead of the list reshuffling every refresh.
fn upsert_by<T>(list: &mut Vec<T>, item: T, key: impl Fn(&T) -> &str) {
    match list.iter_mut().find(|existing| key(existing) == key(&item)) {
        Some(existing) => *existing = item,
        None => list.push(item),
    }
}

/// Accumulates `StatusField`s as they stream in from `collect_streaming`,
/// one `Option` field per single-value `StatusField` variant and one `Vec`
/// (upserted by target, not replaced wholesale) per target-list variant.
/// Shared by every UI (`netcheck-tui`, `netcheck-gui`) so a new
/// `StatusField` variant only needs its `merge`/`has_any` arm added once
/// instead of once per UI.
#[derive(Debug, Clone, Default)]
pub struct PartialStatus {
    pub interfaces: Option<Vec<Interface>>,
    pub vpn: Option<VpnStatus>,
    pub split_dns: Option<bool>,
    pub resolvers: Option<Vec<Resolver>>,
    pub reachability: Vec<PingResult>,
    pub reachability_v6: Vec<PingResult>,
    pub resolution: Vec<ResolutionResult>,
    pub domain_reachability: Vec<ConnectResult>,
    pub gateway_reachability: Vec<PingResult>,
    pub nameserver_reachability: Vec<PingResult>,
    pub nameserver_connect: Vec<ConnectResult>,
    pub proxy: Option<ProxyConfig>,
    pub wifi_identity: Option<WifiIdentity>,
    pub wifi_radio: Option<WifiRadio>,
    pub ip_stack: Option<IpStack>,
    pub captive_portal: Option<CaptivePortalStatus>,
    /// Groups whose target list has fully drained at least once — only
    /// tracked for the four groups `confidence` needs a completeness signal
    /// for (see `StatusField::GroupComplete`).
    completed_groups: std::collections::HashSet<ProbeGroup>,
}

impl PartialStatus {
    pub fn merge(&mut self, field: StatusField) {
        match field {
            StatusField::Interfaces(v) => self.interfaces = Some(v),
            StatusField::Vpn(v) => self.vpn = Some(v),
            StatusField::Resolvers(v) => self.resolvers = Some(v),
            StatusField::SplitDns(v) => self.split_dns = Some(v),
            StatusField::Ping { group, result } => {
                let list = match group {
                    ProbeGroup::Reachability => &mut self.reachability,
                    ProbeGroup::ReachabilityV6 => &mut self.reachability_v6,
                    ProbeGroup::GatewayReachability => &mut self.gateway_reachability,
                    ProbeGroup::NameserverReachability => &mut self.nameserver_reachability,
                    ProbeGroup::DomainReachability
                    | ProbeGroup::NameserverConnect
                    | ProbeGroup::Resolution => {
                        unreachable!("StatusField::Ping is never constructed with a connect/resolution group")
                    }
                };
                upsert_by(list, result, |r| &r.target);
            }
            StatusField::Connect { group, result } => {
                let list = match group {
                    ProbeGroup::DomainReachability => &mut self.domain_reachability,
                    ProbeGroup::NameserverConnect => &mut self.nameserver_connect,
                    ProbeGroup::Reachability
                    | ProbeGroup::ReachabilityV6
                    | ProbeGroup::GatewayReachability
                    | ProbeGroup::NameserverReachability
                    | ProbeGroup::Resolution => {
                        unreachable!("StatusField::Connect is never constructed with a ping/resolution group")
                    }
                };
                upsert_by(list, result, |r| &r.target);
            }
            StatusField::Resolution(v) => upsert_by(&mut self.resolution, v, |r| &r.domain),
            StatusField::GroupComplete(group) => {
                self.completed_groups.insert(group);
            }
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
            || !self.reachability.is_empty()
            || !self.reachability_v6.is_empty()
            || !self.resolution.is_empty()
            || !self.domain_reachability.is_empty()
            || !self.gateway_reachability.is_empty()
            || !self.nameserver_reachability.is_empty()
            || !self.nameserver_connect.is_empty()
            || self.proxy.is_some()
            || self.wifi_identity.is_some()
            || self.wifi_radio.is_some()
            || self.ip_stack.is_some()
            || self.captive_portal.is_some()
    }

    /// `None` until resolution, both reachability probes, domain
    /// reachability, and the captive-portal probe have all *fully drained*
    /// (not just started arriving) — a partial view of only some signal
    /// categories, or a group still mid-stream, would misreport confidence
    /// (e.g. reading `Limited` just because DNS hasn't come back yet, not
    /// because it failed, or reading `Online` before the captive-portal cap
    /// has had a chance to apply).
    pub fn confidence(&self) -> Option<ConnectionConfidence> {
        for group in [
            ProbeGroup::Resolution,
            ProbeGroup::Reachability,
            ProbeGroup::ReachabilityV6,
            ProbeGroup::DomainReachability,
        ] {
            if !self.completed_groups.contains(&group) {
                return None;
            }
        }
        let captive_portal = self.captive_portal.as_ref()?;
        let dns_ok = majority_ok(&self.resolution, |r| r.resolved);
        let ping_ok = majority_ok(&self.reachability, |p| p.reachable)
            || majority_ok(&self.reachability_v6, |p| p.reachable);
        let tcp_ok = majority_ok(&self.domain_reachability, |c| c.reachable);
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

    /// Tallies `field`s into counts keyed by a caller-chosen label, so tests
    /// can assert per-target-item counts (`Ping`/`Connect`/`Resolution`) and
    /// per-group `GroupComplete` coverage without needing exact discriminant
    /// matching for variants that are now sent more than once.
    fn tally(received: &[StatusField]) -> std::collections::HashMap<&'static str, usize> {
        let mut counts = std::collections::HashMap::new();
        for field in received {
            let label: &'static str = match field {
                StatusField::Interfaces(_) => "Interfaces",
                StatusField::Vpn(_) => "Vpn",
                StatusField::Resolvers(_) => "Resolvers",
                StatusField::SplitDns(_) => "SplitDns",
                StatusField::Ping { .. } => "Ping",
                StatusField::Connect { .. } => "Connect",
                StatusField::Resolution(_) => "Resolution",
                StatusField::GroupComplete(_) => "GroupComplete",
                StatusField::Proxy(_) => "Proxy",
                StatusField::WifiIdentity(_) => "WifiIdentity",
                StatusField::WifiRadio(_) => "WifiRadio",
                StatusField::IpStack(_) => "IpStack",
                StatusField::CaptivePortal(_) => "CaptivePortal",
            };
            *counts.entry(label).or_insert(0) += 1;
        }
        counts
    }

    fn group_counts(
        received: &[StatusField],
        variant: impl Fn(&StatusField) -> Option<ProbeGroup>,
    ) -> std::collections::HashMap<ProbeGroup, usize> {
        let mut counts = std::collections::HashMap::new();
        for field in received {
            if let Some(group) = variant(field) {
                *counts.entry(group).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Guards the send side of `collect_streaming`: every single-value
    /// `StatusField` variant must be sent exactly once, every target-list
    /// item must be sent once per target in its group's default target
    /// list (or, for the two dynamic groups — gateway/nameserver IPs,
    /// unknown until runtime — at least be present with a plausible count),
    /// and exactly the four confidence-relevant groups get a
    /// `GroupComplete`. This shells out to real macOS tools (ping, scutil,
    /// system_profiler); it asserts on message counts and variant/group
    /// coverage, not on reachability values, so it's deterministic even
    /// without live network.
    #[test]
    fn collect_streaming_sends_every_status_field_and_probe_group() {
        let (tx, rx) = mpsc::channel();
        collect_streaming(tx);
        let received: Vec<StatusField> = rx.into_iter().collect();

        let counts = tally(&received);
        for label in [
            "Interfaces",
            "Vpn",
            "Resolvers",
            "SplitDns",
            "Proxy",
            "WifiIdentity",
            "WifiRadio",
            "IpStack",
            "CaptivePortal",
        ] {
            assert_eq!(
                counts.get(label).copied().unwrap_or(0),
                1,
                "expected exactly one {label}, got {:?}: {received:?}",
                counts.get(label)
            );
        }

        let ping_counts = group_counts(&received, |f| match f {
            StatusField::Ping { group, .. } => Some(*group),
            _ => None,
        });
        assert_eq!(
            ping_counts.get(&ProbeGroup::Reachability).copied(),
            Some(DEFAULT_PING_TARGETS.len()),
            "expected one Ping per Reachability target"
        );
        assert_eq!(
            ping_counts.get(&ProbeGroup::ReachabilityV6).copied(),
            Some(DEFAULT_PING_TARGETS_V6.len()),
            "expected one Ping per ReachabilityV6 target"
        );
        // Gateway/nameserver target counts are runtime-dependent (however
        // many gateway/resolver IPs this machine has, possibly zero) — just
        // assert the groups don't leak into each other's counts.
        for group in [
            ProbeGroup::DomainReachability,
            ProbeGroup::NameserverConnect,
            ProbeGroup::Resolution,
        ] {
            assert_eq!(ping_counts.get(&group), None, "{group:?} is not a Ping group");
        }

        let connect_counts = group_counts(&received, |f| match f {
            StatusField::Connect { group, .. } => Some(*group),
            _ => None,
        });
        assert_eq!(
            connect_counts.get(&ProbeGroup::DomainReachability).copied(),
            Some(DEFAULT_RESOLUTION_TARGETS.len()),
            "expected one Connect per DomainReachability target"
        );

        assert_eq!(
            counts.get("Resolution").copied(),
            Some(DEFAULT_RESOLUTION_TARGETS.len()),
            "expected one Resolution message per resolution target"
        );

        let complete_groups: HashSet<ProbeGroup> = received
            .iter()
            .filter_map(|f| match f {
                StatusField::GroupComplete(g) => Some(*g),
                _ => None,
            })
            .collect();
        assert_eq!(
            complete_groups,
            HashSet::from([
                ProbeGroup::Reachability,
                ProbeGroup::ReachabilityV6,
                ProbeGroup::Resolution,
                ProbeGroup::DomainReachability,
            ]),
            "expected GroupComplete for exactly the four confidence-relevant groups"
        );
    }

    /// Mirrors `collect_streaming_sends_every_status_field_and_probe_group`
    /// for the confidence-only path: it must send only `Ping`/`Connect`/
    /// `Resolution` items for the four confidence groups plus their
    /// `GroupComplete` markers, and nothing from the static-config probes
    /// it's meant to skip.
    #[test]
    fn collect_confidence_streaming_sends_only_the_four_confidence_groups() {
        let (tx, rx) = mpsc::channel();
        collect_confidence_streaming(tx);
        let received: Vec<StatusField> = rx.into_iter().collect();

        for label in [
            "Interfaces",
            "Vpn",
            "Resolvers",
            "SplitDns",
            "Proxy",
            "WifiIdentity",
            "WifiRadio",
            "IpStack",
            "CaptivePortal",
        ] {
            assert_eq!(
                tally(&received).get(label).copied().unwrap_or(0),
                0,
                "confidence-only streaming must not send {label}"
            );
        }

        assert_eq!(
            received.len(),
            DEFAULT_PING_TARGETS.len()
                + DEFAULT_PING_TARGETS_V6.len()
                + DEFAULT_RESOLUTION_TARGETS.len() * 2
                + 4,
            "expected one message per target across the four confidence groups, plus 4 GroupComplete: {received:?}"
        );

        let complete_groups: HashSet<ProbeGroup> = received
            .iter()
            .filter_map(|f| match f {
                StatusField::GroupComplete(g) => Some(*g),
                _ => None,
            })
            .collect();
        assert_eq!(
            complete_groups,
            HashSet::from([
                ProbeGroup::Reachability,
                ProbeGroup::ReachabilityV6,
                ProbeGroup::Resolution,
                ProbeGroup::DomainReachability,
            ]),
        );
    }

    /// Guards `schema/status-field.schema.json` (the wire format the README
    /// links to, and what `netcheck schema` prints) against drifting from
    /// `StatusField` itself — a schema hand-generated once and then left
    /// alone would silently go stale the next time a field/variant changes.
    /// Regenerate with `netcheck schema > schema/status-field.schema.json`
    /// (or `mise run schema:gen`) if this fails.
    #[test]
    fn committed_schema_matches_status_field() {
        let generated = serde_json::to_string_pretty(&status_field_schema()).unwrap();
        let committed = include_str!("../../../schema/status-field.schema.json");
        assert_eq!(
            generated.trim_end(),
            committed.trim_end(),
            "schema/status-field.schema.json is stale — regenerate with \
             `netcheck schema > schema/status-field.schema.json` (or `mise run schema:gen`) \
             and commit the result"
        );
    }

    #[test]
    fn watch_command_refresh_decodes_from_the_documented_wire_shape() {
        // A bare `"Refresh"` string is serde's canonical representation of
        // a data-less enum variant (confirmed via `serde_json::to_string`
        // — this is what StatusFetcher.swift sends) — guarded explicitly
        // since nothing else exercises this deserialization path (unlike
        // StatusField, which every `collect_streaming` test round-trips
        // through JSON already).
        let decoded: WatchCommand = serde_json::from_str(r#""Refresh""#).unwrap();
        assert_eq!(decoded, WatchCommand::Refresh);
    }

    #[test]
    fn watch_command_also_accepts_the_externally_tagged_object_form() {
        // serde's deserializer is lenient for a data-less variant and
        // accepts `{"Refresh":null}` too, matching StatusField's other
        // (data-carrying) variants' shape — not what's documented as the
        // canonical wire form, but worth guarding since nothing else would
        // catch this leniency regressing.
        let decoded: WatchCommand = serde_json::from_str(r#"{"Refresh":null}"#).unwrap();
        assert_eq!(decoded, WatchCommand::Refresh);
    }

    #[test]
    fn watch_command_rejects_unrecognized_input() {
        assert!(serde_json::from_str::<WatchCommand>(r#"{"Bogus":null}"#).is_err());
        assert!(serde_json::from_str::<WatchCommand>("not json").is_err());
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
            gateway_reachability: Vec::new(),
            nameserver_reachability: Vec::new(),
            nameserver_connect: Vec::new(),
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
            resolution: resolution_results(&[true; 7]),
            reachability: ping_results(&[true; 7]),
            reachability_v6: ping_results(&[true; 5]),
            domain_reachability: connect_results(&[true; 7]),
            completed_groups: HashSet::from([
                ProbeGroup::Resolution,
                ProbeGroup::Reachability,
                ProbeGroup::ReachabilityV6,
                ProbeGroup::DomainReachability,
            ]),
            ..Default::default()
        };
        assert_eq!(partial.confidence(), None);

        partial.captive_portal = Some(captive_portal::CaptivePortalStatus::Detected);
        assert_eq!(partial.confidence(), Some(ConnectionConfidence::Limited));
    }

    #[test]
    fn partial_status_confidence_waits_for_each_group_to_complete() {
        let mut partial = PartialStatus {
            resolution: resolution_results(&[true; 7]),
            reachability: ping_results(&[true; 7]),
            reachability_v6: ping_results(&[true; 5]),
            domain_reachability: connect_results(&[true; 7]),
            captive_portal: Some(captive_portal::CaptivePortalStatus::Clear),
            ..Default::default()
        };
        assert_eq!(
            partial.confidence(),
            None,
            "no group has been marked complete yet"
        );

        partial
            .completed_groups
            .insert(ProbeGroup::Reachability);
        assert_eq!(
            partial.confidence(),
            None,
            "still missing ReachabilityV6/Resolution/DomainReachability completion"
        );

        partial.completed_groups.extend([
            ProbeGroup::ReachabilityV6,
            ProbeGroup::Resolution,
            ProbeGroup::DomainReachability,
        ]);
        assert_eq!(partial.confidence(), Some(ConnectionConfidence::Online));
    }
}
