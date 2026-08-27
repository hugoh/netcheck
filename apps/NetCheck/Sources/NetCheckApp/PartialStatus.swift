import Foundation
import SwiftUI

/// Mirrors `netstatus::dns::vpn_scoped_domains` (Rust) — domains only
/// resolvable via a VPN tunnel's own resolver.
func vpnScopedDomains(_ resolvers: [Resolver]) -> [String] {
    var domains: [String] = []
    for r in resolvers {
        guard r.scoped, let ifName = r.ifName, ifName.hasPrefix("utun") else { continue }
        guard let domain = r.domain ?? r.searchDomains.first else { continue }
        if !domains.contains(domain) {
            domains.append(domain)
        }
    }
    return domains
}

/// One line of `netcheck stream`'s NDJSON output — exactly one field is
/// non-nil per envelope, matching Rust's `StatusField` enum (serde's
/// default externally-tagged representation: `{"VariantName": <data>}`).
/// `ping`/`connect`/`resolution` arrive per-target, not per-list — a slow or
/// unreachable target no longer holds up the rest of its list from
/// appearing (see `PartialNetworkStatus.merge`).
struct StatusFieldEnvelope: Decodable {
    let interfaces: [NetInterface]?
    let vpn: VpnStatus?
    let resolvers: [Resolver]?
    let splitDns: Bool?
    let ping: PingUpdate?
    let connect: ConnectUpdate?
    let resolution: ResolutionResult?
    let groupComplete: ProbeGroup?
    let proxy: ProxyConfig?
    let wifiIdentity: WifiIdentity?
    let wifiRadio: WifiRadio?
    let ipStack: IpStack?
    let captivePortal: CaptivePortalStatus?

    enum CodingKeys: String, CodingKey {
        case interfaces = "Interfaces"
        case vpn = "Vpn"
        case resolvers = "Resolvers"
        case splitDns = "SplitDns"
        case ping = "Ping"
        case connect = "Connect"
        case resolution = "Resolution"
        case groupComplete = "GroupComplete"
        case proxy = "Proxy"
        case wifiIdentity = "WifiIdentity"
        case wifiRadio = "WifiRadio"
        case ipStack = "IpStack"
        case captivePortal = "CaptivePortal"
    }
}

/// One line of the `netcheck stream`/`watch` stdout wire protocol (v1) —
/// exactly one field is non-nil per envelope, matching Rust's `StreamEvent`
/// enum. `field` carries a `StatusFieldEnvelope` plus the `generation` of
/// the check that produced it; `checkStarted`/`checkComplete` bracket each
/// check; `hello` is always the first line; `heartbeat` is `watch`-only
/// liveness; `error` is a bad stdin command or a probe failure.
struct StreamEventEnvelope: Decodable {
    let hello: Hello?
    let checkStarted: CheckStarted?
    let field: FieldEvent?
    let checkComplete: GenerationOnly?
    let heartbeat: Heartbeat?
    let error: WireError?

    enum CodingKeys: String, CodingKey {
        case hello = "Hello"
        case checkStarted = "CheckStarted"
        case field = "Field"
        case checkComplete = "CheckComplete"
        case heartbeat = "Heartbeat"
        case error = "Error"
    }

    /// Decoded with the caller's `.convertFromSnakeCase` strategy, so the
    /// wire's `protocol_version` / `config_watcher` map to these by name —
    /// no explicit `CodingKeys` (which the strategy would fight).
    struct Hello: Decodable {
        let protocolVersion: Int
        let configWatcher: String
    }

    struct CheckStarted: Decodable {
        let generation: Int
        let scope: String
        let trigger: String
        let token: String?
    }

    struct FieldEvent: Decodable {
        let generation: Int
        let field: StatusFieldEnvelope
    }

    struct GenerationOnly: Decodable {
        let generation: Int
    }

    struct Heartbeat: Decodable {}

    struct WireError: Decodable {
        let message: String
        let fatal: Bool
    }
}

/// Progressively-filled network status, mirroring netcheck-tui/-gui's
/// `PartialStatus`. Single-value fields start `nil` and are merged in as
/// each `StatusFieldEnvelope` line arrives; a refresh never blanks a field
/// back to `nil` — only a new value replaces the old one. Target-list
/// fields (`reachability`, `resolution`, ...) start empty and are upserted
/// by target as each item streams in, so fast targets show up immediately
/// instead of waiting on the slowest one in their group.
struct PartialNetworkStatus {
    var interfaces: [NetInterface]?
    var vpn: VpnStatus?
    var splitDns: Bool?
    var resolvers: [Resolver]?
    var reachability: [PingResult] = []
    var reachabilityV6: [PingResult] = []
    var resolution: [ResolutionResult] = []
    var domainReachability: [ConnectResult] = []
    var proxy: ProxyConfig?
    var wifiIdentity: WifiIdentity?
    var wifiRadio: WifiRadio?
    var ipStack: IpStack?
    var captivePortal: CaptivePortalStatus?
    /// Groups whose target list has fully drained at least once — only
    /// tracked for the four groups `confidence` needs a completeness signal
    /// for (see `StatusFieldEnvelope.groupComplete`).
    var completedGroups: Set<ProbeGroup> = []
    /// The `merge` generation each of those four groups most recently
    /// completed at. Since a stdin-triggered refresh against an
    /// already-running `watch` process has no "the process exited" signal
    /// to mark it done (unlike the auto-refresh-off one-shot `stream`
    /// fallback), `isRefreshComplete` uses this instead — the same
    /// completeness signal `confidence` itself waits on.
    private(set) var groupCompleteGeneration: [ProbeGroup: Int] = [:]
    /// The `merge` generation each reachability/resolution row last got a
    /// fresh value at, keyed by `"<group>:<target>"` — lets the UI show a
    /// spinner next to a specific row still waiting on the refresh in
    /// progress, instead of just a single global "refreshing" indicator.
    /// Not tracked for single-value fields or the groups this UI doesn't
    /// render (gateway/nameserver reachability).
    private(set) var rowGeneration: [String: Int] = [:]

    var hasAny: Bool {
        interfaces != nil || vpn != nil || resolvers != nil || splitDns != nil
            || !reachability.isEmpty || !reachabilityV6.isEmpty || !resolution.isEmpty
            || !domainReachability.isEmpty || proxy != nil || wifiIdentity != nil
            || wifiRadio != nil || ipStack != nil || captivePortal != nil
    }

    /// True if `key`'s row hasn't been refreshed as recently as `generation`
    /// — i.e. it's still showing a value from before the in-progress
    /// refresh started. Keys are `"<group>:<target>"`, e.g.
    /// `"reachability:1.1.1.1"` or `"resolution:google.com"`.
    func isPending(_ key: String, asOf generation: Int) -> Bool {
        (rowGeneration[key] ?? -1) < generation
    }

    /// True once all four confidence-relevant groups have completed at or
    /// after `generation` — see `groupCompleteGeneration`.
    func isRefreshComplete(asOf generation: Int) -> Bool {
        let requiredGroups: Set<ProbeGroup> = [.resolution, .reachability, .reachabilityV6, .domainReachability]
        return requiredGroups.allSatisfy { (groupCompleteGeneration[$0] ?? -1) >= generation }
    }

    /// Replaces the entry in `list` matching `item` by `key`, or appends it
    /// if no entry matches — keeps a target's position stable across
    /// re-runs instead of the list reshuffling every refresh. Also records
    /// `generation` as this row's freshness stamp for `isPending`. A static
    /// function taking both `inout` params explicitly, rather than a
    /// mutating method on `self` also reaching for `&self.reachability`
    /// etc. — the latter is an exclusivity violation (mutating self while
    /// holding an inout to one of its own stored properties).
    private static func upsert<T>(
        _ list: inout [T],
        _ rowGeneration: inout [String: Int],
        _ item: T,
        key: (T) -> String,
        rowKey: String,
        generation: Int
    ) {
        if let idx = list.firstIndex(where: { key($0) == key(item) }) {
            list[idx] = item
        } else {
            list.append(item)
        }
        rowGeneration[rowKey] = generation
    }

    /// `generation` is the wire generation from the enclosing
    /// `StreamEvent::Field` (monotonic per `netcheck watch` process),
    /// stamped onto each row so `isPending` can tell "updated before the
    /// in-flight refresh started" from "updated by it". The auto-refresh-off
    /// one-shot `stream` fallback passes its own local counter instead.
    mutating func merge(_ envelope: StatusFieldEnvelope, generation: Int) {
        if let v = envelope.interfaces { interfaces = v }
        if let v = envelope.vpn { vpn = v }
        if let v = envelope.resolvers { resolvers = v }
        if let v = envelope.splitDns { splitDns = v }
        if let v = envelope.ping {
            switch v.group {
            case .reachability:
                Self.upsert(&reachability, &rowGeneration, v.result, key: { $0.target },
                            rowKey: "reachability:\(v.result.target)", generation: generation)
            case .reachabilityV6:
                Self.upsert(&reachabilityV6, &rowGeneration, v.result, key: { $0.target },
                            rowKey: "reachabilityV6:\(v.result.target)", generation: generation)
            case .gatewayReachability, .nameserverReachability:
                break // not surfaced in this UI
            case .domainReachability, .nameserverConnect, .resolution:
                break // never sent as a Ping update
            }
        }
        if let v = envelope.connect, v.group == .domainReachability {
            Self.upsert(&domainReachability, &rowGeneration, v.result, key: { $0.target },
                        rowKey: "domainReachability:\(v.result.target)", generation: generation)
        }
        if let v = envelope.resolution {
            Self.upsert(&resolution, &rowGeneration, v, key: { $0.domain },
                        rowKey: "resolution:\(v.domain)", generation: generation)
        }
        if let g = envelope.groupComplete {
            completedGroups.insert(g)
            groupCompleteGeneration[g] = generation
        }
        if let v = envelope.proxy { proxy = v }
        if let v = envelope.wifiIdentity { wifiIdentity = v }
        if let v = envelope.wifiRadio { wifiRadio = v }
        if let v = envelope.ipStack { ipStack = v }
        if let v = envelope.captivePortal { captivePortal = v }
    }
}

/// How confident netcheck is that the machine has a working internet
/// connection. Mirrors `netstatus::status::confidence_from` (Rust) so
/// both UIs agree on the tiering — see `vpnScopedDomains` above for the
/// same mirroring pattern.
enum ConnectionConfidence {
    case online
    case limited
    case offline
}

extension ConnectionConfidence {
    var label: String {
        switch self {
        case .online: return "Online"
        case .limited: return "Limited"
        case .offline: return "Offline"
        }
    }

    var icon: String {
        switch self {
        case .online: return "checkmark.circle.fill"
        case .limited: return "exclamationmark.circle.fill"
        case .offline: return "xmark.circle.fill"
        }
    }

    var color: Color {
        switch self {
        case .online: return .green
        case .limited: return .yellow
        case .offline: return .red
        }
    }
}

/// True if more than half of `results` satisfy `isOk`, mirroring Rust's
/// `majority_ok` — a single lucky reply from an otherwise-unreachable
/// target list shouldn't count as a category "working". Empty lists are
/// never ok.
private func majorityOk<T>(_ results: [T], _ isOk: (T) -> Bool) -> Bool {
    let n = results.count
    return n > 0 && results.filter(isOk).count * 2 > n
}

/// Rolls up whether DNS resolution, ICMP reachability, and TCP connect
/// each had a majority of their probed targets succeed into a single
/// confidence tier, mirroring Rust's `confidence_from`.
private func confidenceFrom(dnsOk: Bool, pingOk: Bool, tcpOk: Bool) -> ConnectionConfidence {
    switch [dnsOk, pingOk, tcpOk].filter({ $0 }).count {
    case 3, 2: return .online
    case 1: return .limited
    default: return .offline
    }
}

/// Caps `confidence` at `.limited` when `captivePortal` confirms a portal
/// is present, mirroring Rust's `cap_for_captive_portal`. `.unknown` is not
/// evidence of a portal, so it doesn't trigger the cap.
private func capForCaptivePortal(
    _ confidence: ConnectionConfidence,
    _ captivePortal: CaptivePortalStatus
) -> ConnectionConfidence {
    if captivePortal == .detected && confidence == .online {
        return .limited
    }
    return confidence
}

extension PartialNetworkStatus {
    /// `nil` until resolution, both reachability probes, domain
    /// reachability, and the captive-portal probe have all *fully drained*
    /// (not just started arriving) — a group still mid-stream would
    /// misreport confidence the same way a missing category would, and a
    /// missing captive-portal reading would skip the cap below.
    var confidence: ConnectionConfidence? {
        let requiredGroups: Set<ProbeGroup> = [.resolution, .reachability, .reachabilityV6, .domainReachability]
        guard requiredGroups.isSubset(of: completedGroups) else { return nil }
        guard let captivePortal else { return nil }
        let dnsOk = majorityOk(resolution) { $0.resolved }
        let pingOk = majorityOk(reachability) { $0.reachable } || majorityOk(reachabilityV6) { $0.reachable }
        let tcpOk = majorityOk(domainReachability) { $0.reachable }
        return capForCaptivePortal(confidenceFrom(dnsOk: dnsOk, pingOk: pingOk, tcpOk: tcpOk), captivePortal)
    }
}
