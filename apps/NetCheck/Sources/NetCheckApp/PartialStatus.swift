import Foundation

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
struct StatusFieldEnvelope: Decodable {
    let interfaces: [NetInterface]?
    let vpn: VpnStatus?
    let resolvers: [Resolver]?
    let splitDns: Bool?
    let reachability: [PingResult]?
    let reachabilityV6: [PingResult]?
    let resolution: [ResolutionResult]?
    let domainReachability: [ConnectResult]?
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
        case reachability = "Reachability"
        case reachabilityV6 = "ReachabilityV6"
        case resolution = "Resolution"
        case domainReachability = "DomainReachability"
        case proxy = "Proxy"
        case wifiIdentity = "WifiIdentity"
        case wifiRadio = "WifiRadio"
        case ipStack = "IpStack"
        case captivePortal = "CaptivePortal"
    }
}

/// Progressively-filled network status, mirroring netcheck-tui/-gui's
/// `PartialStatus`. Fields start `nil` and are merged in as each
/// `StatusFieldEnvelope` line arrives; a refresh never blanks a field back
/// to `nil` — only a new value replaces the old one.
struct PartialNetworkStatus {
    var interfaces: [NetInterface]?
    var vpn: VpnStatus?
    var splitDns: Bool?
    var resolvers: [Resolver]?
    var reachability: [PingResult]?
    var reachabilityV6: [PingResult]?
    var resolution: [ResolutionResult]?
    var domainReachability: [ConnectResult]?
    var proxy: ProxyConfig?
    var wifiIdentity: WifiIdentity?
    var wifiRadio: WifiRadio?
    var ipStack: IpStack?
    var captivePortal: CaptivePortalStatus?

    var hasAny: Bool {
        interfaces != nil || vpn != nil || resolvers != nil || splitDns != nil
            || reachability != nil || reachabilityV6 != nil || resolution != nil
            || domainReachability != nil || proxy != nil || wifiIdentity != nil
            || wifiRadio != nil || ipStack != nil || captivePortal != nil
    }

    mutating func merge(_ envelope: StatusFieldEnvelope) {
        if let v = envelope.interfaces { interfaces = v }
        if let v = envelope.vpn { vpn = v }
        if let v = envelope.resolvers { resolvers = v }
        if let v = envelope.splitDns { splitDns = v }
        if let v = envelope.reachability { reachability = v }
        if let v = envelope.reachabilityV6 { reachabilityV6 = v }
        if let v = envelope.resolution { resolution = v }
        if let v = envelope.domainReachability { domainReachability = v }
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
    /// reachability have all arrived.
    var confidence: ConnectionConfidence? {
        guard let resolution, let reachability, let reachabilityV6, let domainReachability else {
            return nil
        }
        let dnsOk = resolution.contains { $0.resolved }
        let pingOk = reachability.contains { $0.reachable } || reachabilityV6.contains { $0.reachable }
        let tcpOk = domainReachability.contains { $0.reachable }
        return confidenceFrom(dnsOk: dnsOk, pingOk: pingOk, tcpOk: tcpOk)
    }
}
