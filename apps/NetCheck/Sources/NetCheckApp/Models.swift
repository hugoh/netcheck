import Foundation

struct NetInterface: Codable, Identifiable {
    var id: String { name }
    let name: String
    let up: Bool
    let loopback: Bool
    let addresses: [String]
}

struct VpnStatus: Codable {
    let tunnels: [String]
    let primaryInterface: String?
    let connected: Bool
    let splitTunnel: Bool
    let routedSubnets: [String]?
}

struct Resolver: Codable, Identifiable {
    // `domain` + `ifName` alone can collide: the systemwide-default entry
    // for the primary service always carries `ifName == nil`, and a scoped
    // entry whose own interface-name lookup fails also reports `ifName ==
    // nil` — if both share a domain, `scoped` and `nameservers` are what
    // still tell them apart.
    var id: String {
        (domain ?? searchDomains.first ?? "*") + (ifName ?? "") + (scoped ? "s" : "u")
            + nameservers.joined(separator: ",")
    }
    let domain: String?
    let searchDomains: [String]
    let nameservers: [String]
    let ifIndex: UInt32?
    let ifName: String?
    let scoped: Bool
    let reachable: Bool
}

struct PingResult: Codable, Identifiable {
    var id: String { target }
    let target: String
    let reachable: Bool
    let rttMs: Double?
}

struct ResolutionResult: Codable, Identifiable {
    var id: String { domain }
    let domain: String
    let resolved: Bool
    let addresses: [String]
    let durationMs: Double?
}

struct ConnectResult: Codable, Identifiable {
    var id: String { "\(target):\(port)" }
    let target: String
    let port: UInt16
    let reachable: Bool
    let rttMs: Double?
}

struct ProxyEndpoint: Codable {
    let enabled: Bool
    let host: String?
    let port: UInt16?
}

struct ProxyConfig: Codable {
    let http: ProxyEndpoint
    let https: ProxyEndpoint
    let socks: ProxyEndpoint
    let pacUrl: String?
    let exceptions: [String]
}

struct WifiStatus: Codable {
    let connected: Bool
    let ssid: String?
    let channel: String?
    let signalDbm: Int?
    let noiseDbm: Int?
    let security: String?
    let phyMode: String?
}

/// SSID/connected-state, the slow half of Wi-Fi status (system_profiler).
struct WifiIdentity: Codable {
    let connected: Bool
    let ssid: String?
}

/// Channel/signal/noise/security/PHY-mode, the fast half (CoreWLAN).
struct WifiRadio: Codable {
    let channel: String?
    let signalDbm: Int?
    let noiseDbm: Int?
    let security: String?
    let phyMode: String?
}

enum IpStack: String, Codable {
    case ipv4Only = "Ipv4Only"
    case ipv6Only = "Ipv6Only"
    case dualStack = "DualStack"
    case none = "None"
}

enum CaptivePortalStatus: String, Codable {
    case clear = "Clear"
    case detected = "Detected"
    case unknown = "Unknown"
}

struct NetworkStatus: Codable {
    let interfaces: [NetInterface]
    let vpn: VpnStatus
    let resolvers: [Resolver]
    let splitDns: Bool
    let reachability: [PingResult]
    let reachabilityV6: [PingResult]?
    let resolution: [ResolutionResult]
    let domainReachability: [ConnectResult]
    let proxy: ProxyConfig?
    let wifi: WifiStatus?
    let ipStack: IpStack?
}
