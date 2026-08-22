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

    var hasAny: Bool {
        interfaces != nil || vpn != nil || resolvers != nil || splitDns != nil
            || reachability != nil || reachabilityV6 != nil || resolution != nil
            || domainReachability != nil || proxy != nil || wifiIdentity != nil
            || wifiRadio != nil || ipStack != nil
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
    }
}
