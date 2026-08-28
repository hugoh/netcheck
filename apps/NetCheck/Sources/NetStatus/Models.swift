import Foundation

// The canonical model types for the network-status engine. `Codable` so
// the CLI can emit them as JSON (`.convertToSnakeCase` at the encoder).

public struct NetInterface: Codable, Identifiable, Sendable, Equatable {
    public var id: String {
        name
    }

    public let name: String
    public let up: Bool
    public let loopback: Bool
    public let addresses: [String]

    public init(name: String, up: Bool, loopback: Bool, addresses: [String]) {
        self.name = name
        self.up = up
        self.loopback = loopback
        self.addresses = addresses
    }
}

public struct VpnStatus: Codable, Sendable, Equatable {
    public let tunnels: [String]
    public let primaryInterface: String?
    public let connected: Bool
    public let splitTunnel: Bool
    public let routedSubnets: [String]

    public init(
        tunnels: [String], primaryInterface: String?, connected: Bool,
        splitTunnel: Bool, routedSubnets: [String]
    ) {
        self.tunnels = tunnels
        self.primaryInterface = primaryInterface
        self.connected = connected
        self.splitTunnel = splitTunnel
        self.routedSubnets = routedSubnets
    }
}

public struct Resolver: Codable, Identifiable, Sendable, Equatable {
    public var id: String {
        (domain ?? searchDomains.first ?? "*") + (ifName ?? "") + (scoped ? "s" : "u")
            + nameservers.joined(separator: ",")
    }

    public let domain: String?
    public let searchDomains: [String]
    public let nameservers: [String]
    public let ifIndex: UInt32?
    public let ifName: String?
    public let scoped: Bool
    public let reachable: Bool

    public init(
        domain: String?, searchDomains: [String], nameservers: [String],
        ifIndex: UInt32?, ifName: String?, scoped: Bool, reachable: Bool
    ) {
        self.domain = domain
        self.searchDomains = searchDomains
        self.nameservers = nameservers
        self.ifIndex = ifIndex
        self.ifName = ifName
        self.scoped = scoped
        self.reachable = reachable
    }
}

public struct PingResult: Codable, Identifiable, Sendable, Equatable {
    public var id: String {
        target
    }

    public let target: String
    public let reachable: Bool
    public let rttMs: Double?

    public init(target: String, reachable: Bool, rttMs: Double?) {
        self.target = target
        self.reachable = reachable
        self.rttMs = rttMs
    }
}

public struct ResolutionResult: Codable, Identifiable, Sendable, Equatable {
    public var id: String {
        domain
    }

    public let domain: String
    public let resolved: Bool
    public let addresses: [String]
    public let durationMs: Double?

    public init(domain: String, resolved: Bool, addresses: [String], durationMs: Double?) {
        self.domain = domain
        self.resolved = resolved
        self.addresses = addresses
        self.durationMs = durationMs
    }
}

public struct ConnectResult: Codable, Identifiable, Sendable, Equatable {
    public var id: String {
        "\(target):\(port)"
    }

    public let target: String
    public let port: UInt16
    public let reachable: Bool
    public let rttMs: Double?

    public init(target: String, port: UInt16, reachable: Bool, rttMs: Double?) {
        self.target = target
        self.port = port
        self.reachable = reachable
        self.rttMs = rttMs
    }
}

public struct ProxyEndpoint: Codable, Sendable, Equatable {
    public let enabled: Bool
    public let host: String?
    public let port: UInt16?

    public init(enabled: Bool, host: String?, port: UInt16?) {
        self.enabled = enabled
        self.host = host
        self.port = port
    }
}

public struct ProxyConfig: Codable, Sendable, Equatable {
    public let http: ProxyEndpoint
    public let https: ProxyEndpoint
    public let socks: ProxyEndpoint
    public let pacUrl: String?
    public let exceptions: [String]

    public init(
        http: ProxyEndpoint, https: ProxyEndpoint, socks: ProxyEndpoint,
        pacUrl: String?, exceptions: [String]
    ) {
        self.http = http
        self.https = https
        self.socks = socks
        self.pacUrl = pacUrl
        self.exceptions = exceptions
    }
}

/// SSID/connected-state, the slow half of Wi-Fi status (system_profiler).
public struct WifiIdentity: Codable, Sendable, Equatable {
    public let connected: Bool
    public let ssid: String?

    public init(connected: Bool, ssid: String?) {
        self.connected = connected
        self.ssid = ssid
    }
}

/// Channel/signal/noise/security/PHY-mode, the fast half (CoreWLAN).
public struct WifiRadio: Codable, Sendable, Equatable {
    public let channel: String?
    public let signalDbm: Int?
    public let noiseDbm: Int?
    public let security: String?
    public let phyMode: String?

    public init(
        channel: String?, signalDbm: Int?, noiseDbm: Int?,
        security: String?, phyMode: String?
    ) {
        self.channel = channel
        self.signalDbm = signalDbm
        self.noiseDbm = noiseDbm
        self.security = security
        self.phyMode = phyMode
    }
}

public struct WifiStatus: Codable, Sendable, Equatable {
    public let connected: Bool
    public let ssid: String?
    public let channel: String?
    public let signalDbm: Int?
    public let noiseDbm: Int?
    public let security: String?
    public let phyMode: String?

    // The memberwise initializer is synthesized; `init(identity:radio:)`
    // lives in Wifi.swift.
}

public enum IpStack: String, Codable, Sendable {
    case ipv4Only = "ipv4_only"
    case ipv6Only = "ipv6_only"
    case dualStack = "dual_stack"
    case none
}

/// The OS's own verdict on the primary network path, mirroring
/// `NWPath.status`. Distinct from the ICMP/TCP `reachability` probes —
/// this is "is there a usable path at all", no packets sent.
public enum PathState: String, Codable, Sendable {
    case satisfied
    case unsatisfied
    case requiresConnection = "requires_connection"
}

public enum PathInterfaceType: String, Codable, Sendable {
    case wifi
    case wiredEthernet = "wired_ethernet"
    case cellular
    case loopback
    case other
    case none
}

/// A one-shot snapshot of `NWPath` — the system's picture of the primary
/// route, carrying signal (metered link, Low Data Mode, OS path verdict)
/// that the address/route-table probes can't see.
public struct PathStatus: Codable, Sendable, Equatable {
    public let status: PathState
    public let primaryInterface: PathInterfaceType
    /// The link is metered — a personal hotspot or a cellular connection.
    public let expensive: Bool
    /// Low Data Mode is active for this path.
    public let constrained: Bool
    public let supportsIPv4: Bool
    public let supportsIPv6: Bool

    /// `.convertToSnakeCase` would turn `supportsIPv4` into the misspelt
    /// `supports_i_pv4`; spell those two keys out.
    enum CodingKeys: String, CodingKey {
        case status, primaryInterface, expensive, constrained
        case supportsIPv4 = "supports_ipv4"
        case supportsIPv6 = "supports_ipv6"
    }

    public init(
        status: PathState, primaryInterface: PathInterfaceType,
        expensive: Bool, constrained: Bool, supportsIPv4: Bool, supportsIPv6: Bool
    ) {
        self.status = status
        self.primaryInterface = primaryInterface
        self.expensive = expensive
        self.constrained = constrained
        self.supportsIPv4 = supportsIPv4
        self.supportsIPv6 = supportsIPv6
    }
}

public enum CaptivePortalStatus: String, Codable, Sendable {
    case clear
    case detected
    case unknown
}

/// Whether the machine has working internet access, classified from
/// independent probe signals rather than any single check. `.limited`
/// covers "connected but not really online" — a captive portal, or only
/// one of DNS/ICMP/TCP getting through.
public enum Connectivity: String, Codable, Sendable {
    case online
    case limited
    case offline
}

/// A full snapshot of the machine's network status.
public struct NetworkStatus: Codable, Sendable, Equatable {
    public let interfaces: [NetInterface]
    public let vpn: VpnStatus
    public let resolvers: [Resolver]
    public let splitDns: Bool
    public let reachability: [PingResult]
    public let reachabilityV6: [PingResult]
    public let resolution: [ResolutionResult]
    public let domainReachability: [ConnectResult]
    public let gatewayReachability: [PingResult]
    public let nameserverReachability: [PingResult]
    public let nameserverConnect: [ConnectResult]
    public let proxy: ProxyConfig
    public let wifi: WifiStatus
    public let ipStack: IpStack
    public let path: PathStatus
    public let captivePortal: CaptivePortalStatus

    public init(
        interfaces: [NetInterface], vpn: VpnStatus, resolvers: [Resolver], splitDns: Bool,
        reachability: [PingResult], reachabilityV6: [PingResult],
        resolution: [ResolutionResult], domainReachability: [ConnectResult],
        gatewayReachability: [PingResult], nameserverReachability: [PingResult],
        nameserverConnect: [ConnectResult], proxy: ProxyConfig, wifi: WifiStatus,
        ipStack: IpStack, path: PathStatus, captivePortal: CaptivePortalStatus
    ) {
        self.interfaces = interfaces
        self.vpn = vpn
        self.resolvers = resolvers
        self.splitDns = splitDns
        self.reachability = reachability
        self.reachabilityV6 = reachabilityV6
        self.resolution = resolution
        self.domainReachability = domainReachability
        self.gatewayReachability = gatewayReachability
        self.nameserverReachability = nameserverReachability
        self.nameserverConnect = nameserverConnect
        self.proxy = proxy
        self.wifi = wifi
        self.ipStack = ipStack
        self.path = path
        self.captivePortal = captivePortal
    }

    /// Rolls the three independent signal categories — DNS resolution,
    /// ICMP reachability (v4 or v6), TCP connect — into a connectivity
    /// tier, then caps at `.limited` when a captive portal is confirmed.
    public var connectivity: Connectivity {
        let dnsOk = Self.majorityOk(resolution) { $0.resolved }
        let pingOk = Self.majorityOk(reachability) { $0.reachable }
            || Self.majorityOk(reachabilityV6) { $0.reachable }
        let tcpOk = Self.majorityOk(domainReachability) { $0.reachable }
        let base = Self.classify(dnsOk: dnsOk, pingOk: pingOk, tcpOk: tcpOk)
        if captivePortal == .detected, base == .online {
            return .limited
        }
        return base
    }

    /// True when more than half of `results` satisfy `isOk`. Empty is never ok.
    public static func majorityOk<T>(_ results: [T], _ isOk: (T) -> Bool) -> Bool {
        !results.isEmpty && results.filter(isOk).count * 2 > results.count
    }

    public static func classify(dnsOk: Bool, pingOk: Bool, tcpOk: Bool) -> Connectivity {
        switch [dnsOk, pingOk, tcpOk].filter(\.self).count {
        case 3, 2: .online
        case 1: .limited
        default: .offline
        }
    }
}
