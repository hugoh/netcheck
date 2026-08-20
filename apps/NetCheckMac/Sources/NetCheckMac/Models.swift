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
}

struct Resolver: Codable, Identifiable {
    var id: String { (domain ?? searchDomains.first ?? "*") + (ifName ?? "") }
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

struct NetworkStatus: Codable {
    let interfaces: [NetInterface]
    let vpn: VpnStatus
    let resolvers: [Resolver]
    let splitDns: Bool
    let reachability: [PingResult]
    let resolution: [ResolutionResult]
    let domainReachability: [ConnectResult]
}
