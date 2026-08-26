import NetStatus
import SwiftUI

/// Domains only resolvable via a VPN tunnel's own resolver.
func vpnScopedDomains(_ resolvers: [Resolver]) -> [String] {
    Probe.vpnScopedDomains(resolvers)
}

/// Progressively-filled network status. Single-value fields start `nil` and
/// are merged in as each `StatusField` arrives from
/// `NetStatus.collectStreaming`; a refresh never blanks a field back to
/// `nil`. Target-list fields start empty and are upserted by target so fast
/// targets show up without waiting on the slowest.
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
    var path: PathStatus?
    var captivePortal: CaptivePortalStatus?

    /// Groups whose target list has fully drained at least once.
    private(set) var completedGroups: Set<ProbeGroup> = []
    /// The merge generation each group most recently completed at — the
    /// completeness signal a manual refresh waits on.
    private(set) var groupCompleteGeneration: [ProbeGroup: Int] = [:]
    /// The merge generation each reachability/resolution row last got a
    /// fresh value at, keyed `"<group>:<target>"` — drives the per-row
    /// refresh spinner.
    private(set) var rowGeneration: [String: Int] = [:]

    var hasAny: Bool {
        interfaces != nil || vpn != nil || resolvers != nil || splitDns != nil
            || !reachability.isEmpty || !reachabilityV6.isEmpty || !resolution.isEmpty
            || !domainReachability.isEmpty || proxy != nil || wifiIdentity != nil
            || wifiRadio != nil || ipStack != nil || path != nil || captivePortal != nil
    }

    func isPending(_ key: String, asOf generation: Int) -> Bool {
        (rowGeneration[key] ?? -1) < generation
    }

    func isRefreshComplete(asOf generation: Int) -> Bool {
        Self.confidenceGroups.allSatisfy { (groupCompleteGeneration[$0] ?? -1) >= generation }
    }

    private static let confidenceGroups: Set<ProbeGroup> = [
        .resolution, .reachability, .reachabilityV6, .domainReachability,
    ]

    private mutating func upsert<T>(
        _ list: WritableKeyPath<PartialNetworkStatus, [T]>,
        _ item: T, key: (T) -> String, rowKey: String, generation: Int
    ) {
        if let index = self[keyPath: list].firstIndex(where: { key($0) == key(item) }) {
            self[keyPath: list][index] = item
        } else {
            self[keyPath: list].append(item)
        }
        rowGeneration[rowKey] = generation
    }

    // Flat dispatch over the StatusField cases — breadth, not nested logic.
    // swiftlint:disable:next cyclomatic_complexity
    mutating func merge(_ field: StatusField, generation: Int) {
        switch field {
        case let .interfaces(value): interfaces = value
        case let .vpn(value): vpn = value
        case let .resolvers(value): resolvers = value
        case let .splitDns(value): splitDns = value
        case let .proxy(value): proxy = value
        case let .wifiIdentity(value): wifiIdentity = value
        case let .wifiRadio(value): wifiRadio = value
        case let .ipStack(value): ipStack = value
        case let .path(value): path = value
        case let .captivePortal(value): captivePortal = value
        case let .resolution(result):
            upsert(\.resolution, result, key: { $0.domain },
                   rowKey: "resolution:\(result.domain)", generation: generation)
        case let .ping(group, result):
            mergePing(group, result, generation: generation)
        case let .connect(group, result):
            if group == .domainReachability {
                upsert(\.domainReachability, result, key: { $0.target },
                       rowKey: "domainReachability:\(result.target)", generation: generation)
            }
        case let .groupComplete(group):
            completedGroups.insert(group)
            groupCompleteGeneration[group] = generation
        }
    }

    private mutating func mergePing(_ group: ProbeGroup, _ result: PingResult, generation: Int) {
        switch group {
        case .reachability:
            upsert(\.reachability, result, key: { $0.target },
                   rowKey: "reachability:\(result.target)", generation: generation)
        case .reachabilityV6:
            upsert(\.reachabilityV6, result, key: { $0.target },
                   rowKey: "reachabilityV6:\(result.target)", generation: generation)
        case .gatewayReachability, .nameserverReachability:
            break // not surfaced in this UI
        case .domainReachability, .nameserverConnect, .resolution:
            break // never delivered as a ping
        }
    }
}

/// Whether the machine has working internet — the app's presentation
/// wrapper over `NetStatus.Connectivity`.
enum ConnectionConfidence {
    case online
    case limited
    case offline

    init(_ connectivity: Connectivity) {
        switch connectivity {
        case .online: self = .online
        case .limited: self = .limited
        case .offline: self = .offline
        }
    }

    var label: String {
        switch self {
        case .online: "Online"
        case .limited: "Limited"
        case .offline: "Offline"
        }
    }

    var icon: String {
        switch self {
        case .online: "checkmark.circle.fill"
        case .limited: "exclamationmark.circle.fill"
        case .offline: "xmark.circle.fill"
        }
    }

    var color: Color {
        switch self {
        case .online: .green
        case .limited: .yellow
        case .offline: .red
        }
    }
}

extension PartialNetworkStatus {
    /// `nil` until resolution, both reachability probes, domain
    /// reachability, and the captive-portal probe have all fully drained —
    /// a group still mid-stream would misreport, and a missing
    /// captive-portal reading would skip the cap.
    var confidence: ConnectionConfidence? {
        guard Self.confidenceGroups.isSubset(of: completedGroups), let captivePortal else {
            return nil
        }
        let dnsOk = NetworkStatus.majorityOk(resolution) { $0.resolved }
        let pingOk = NetworkStatus.majorityOk(reachability) { $0.reachable }
            || NetworkStatus.majorityOk(reachabilityV6) { $0.reachable }
        let tcpOk = NetworkStatus.majorityOk(domainReachability) { $0.reachable }
        let base = NetworkStatus.classify(dnsOk: dnsOk, pingOk: pingOk, tcpOk: tcpOk)
        if captivePortal == .detected, base == .online {
            return ConnectionConfidence(.limited)
        }
        return ConnectionConfidence(base)
    }
}
