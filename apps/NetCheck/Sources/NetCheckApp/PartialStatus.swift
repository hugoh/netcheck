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

    var hasAny: Bool {
        interfaces != nil || vpn != nil || resolvers != nil || splitDns != nil
            || !reachability.isEmpty || !reachabilityV6.isEmpty || !resolution.isEmpty
            || !domainReachability.isEmpty || proxy != nil || wifiIdentity != nil
            || wifiRadio != nil || ipStack != nil || captivePortal != nil
    }

    /// Replaces the entry in `list` matching `item` by `key`, or appends it
    /// if no entry matches — keeps a target's position stable across
    /// re-runs instead of the list reshuffling every refresh.
    private func upsert<T>(_ list: inout [T], _ item: T, key: (T) -> String) {
        if let idx = list.firstIndex(where: { key($0) == key(item) }) {
            list[idx] = item
        } else {
            list.append(item)
        }
    }

    mutating func merge(_ envelope: StatusFieldEnvelope) {
        if let v = envelope.interfaces { interfaces = v }
        if let v = envelope.vpn { vpn = v }
        if let v = envelope.resolvers { resolvers = v }
        if let v = envelope.splitDns { splitDns = v }
        if let v = envelope.ping {
            switch v.group {
            case .reachability: upsert(&reachability, v.result, key: { $0.target })
            case .reachabilityV6: upsert(&reachabilityV6, v.result, key: { $0.target })
            case .gatewayReachability, .nameserverReachability:
                break // not surfaced in this UI
            case .domainReachability, .nameserverConnect, .resolution:
                break // never sent as a Ping update
            }
        }
        if let v = envelope.connect, v.group == .domainReachability {
            upsert(&domainReachability, v.result, key: { $0.target })
        }
        if let v = envelope.resolution {
            upsert(&resolution, v, key: { $0.domain })
        }
        if let g = envelope.groupComplete { completedGroups.insert(g) }
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

/// Rolls up whether DNS resolution, ICMP reachability, and TCP connect
/// each had at least one success into a single confidence tier.
private func confidenceFrom(dnsOk: Bool, pingOk: Bool, tcpOk: Bool) -> ConnectionConfidence {
    switch [dnsOk, pingOk, tcpOk].filter({ $0 }).count {
    case 3, 2: return .online
    case 1: return .limited
    default: return .offline
    }
}

extension PartialNetworkStatus {
    /// `nil` until resolution, both reachability probes, and domain
    /// reachability have all *fully drained* (not just started arriving) —
    /// a group still mid-stream would misreport confidence the same way a
    /// missing category would.
    var confidence: ConnectionConfidence? {
        let requiredGroups: Set<ProbeGroup> = [.resolution, .reachability, .reachabilityV6, .domainReachability]
        guard requiredGroups.isSubset(of: completedGroups) else { return nil }
        let dnsOk = resolution.contains { $0.resolved }
        let pingOk = reachability.contains { $0.reachable } || reachabilityV6.contains { $0.reachable }
        let tcpOk = domainReachability.contains { $0.reachable }
        return confidenceFrom(dnsOk: dnsOk, pingOk: pingOk, tcpOk: tcpOk)
    }
}
