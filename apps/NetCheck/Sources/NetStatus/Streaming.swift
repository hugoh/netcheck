import Foundation

/// Which target list a streamed `Ping`/`Connect`/`Resolution` result — or a
/// `groupComplete` marker — belongs to.
public enum ProbeGroup: Sendable, Hashable {
    case reachability
    case reachabilityV6
    case gatewayReachability
    case nameserverReachability
    case domainReachability
    case nameserverConnect
    case resolution
}

/// One probe result, delivered the moment that probe completes — per target
/// for the list probes, not per group.
public enum StatusField: Sendable {
    case interfaces([NetInterface])
    case vpn(VpnStatus)
    case resolvers([Resolver])
    case splitDns(Bool)
    case ping(group: ProbeGroup, result: PingResult)
    case connect(group: ProbeGroup, result: ConnectResult)
    case resolution(ResolutionResult)
    case groupComplete(ProbeGroup)
    case proxy(ProxyConfig)
    case wifiIdentity(WifiIdentity)
    case wifiRadio(WifiRadio)
    case ipStack(IpStack)
    case path(PathStatus)
    case captivePortal(CaptivePortalStatus)
}

/// How much of a check to run. `.full` re-probes everything; `.confidence`
/// runs only what `Connectivity` depends on (the poll-tick safety net).
public enum CollectScope: Sendable {
    case full
    case confidence
}

extension NetStatus {
    /// Runs every probe concurrently and yields each `StatusField` as it
    /// completes rather than waiting for the slowest. Cancelling the
    /// consuming task (or breaking the `for await`) stops the probes.
    public static func collectStreaming(scope: CollectScope = .full) -> AsyncStream<StatusField> {
        AsyncStream { continuation in
            let task = Task {
                await withTaskGroup(of: Void.self) { group in
                    coreSignalProbes(into: continuation, group: &group)
                    group.addTask {
                        await continuation.yield(.captivePortal(Probe.checkCaptivePortal()))
                    }
                    if scope == .full {
                        fullConfigProbes(into: continuation, group: &group)
                    }
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    private static func coreSignalProbes(
        into continuation: AsyncStream<StatusField>.Continuation,
        group: inout TaskGroup<Void>
    ) {
        group.addTask {
            await Probe.pingEach(Defaults.pingTargets) {
                continuation.yield(.ping(group: .reachability, result: $0))
            }
            continuation.yield(.groupComplete(.reachability))
        }
        group.addTask {
            await Probe.pingEach(Defaults.pingTargetsV6) {
                continuation.yield(.ping(group: .reachabilityV6, result: $0))
            }
            continuation.yield(.groupComplete(.reachabilityV6))
        }
        group.addTask {
            await Probe.resolveEach(Defaults.resolutionTargets) {
                continuation.yield(.resolution($0))
            }
            continuation.yield(.groupComplete(.resolution))
        }
        group.addTask {
            await Probe.connectEach(Defaults.resolutionTargets, port: 443) {
                continuation.yield(.connect(group: .domainReachability, result: $0))
            }
            continuation.yield(.groupComplete(.domainReachability))
        }
    }

    private static func fullConfigProbes(
        into continuation: AsyncStream<StatusField>.Continuation,
        group: inout TaskGroup<Void>
    ) {
        group.addTask {
            let interfaces = Probe.listInterfaces()
            continuation.yield(.ipStack(Probe.ipStack(interfaces)))
            continuation.yield(.vpn(Probe.vpnStatus(interfaces)))
            continuation.yield(.interfaces(interfaces))
        }
        group.addTask {
            let resolvers = Probe.listResolvers()
            continuation.yield(.splitDns(Probe.hasSplitDns(resolvers)))
            let nameservers = Probe.nameserverIps(resolvers)
            continuation.yield(.resolvers(resolvers))
            await Probe.pingEach(nameservers) {
                continuation.yield(.ping(group: .nameserverReachability, result: $0))
            }
            await Probe.connectEach(nameservers, port: 53) {
                continuation.yield(.connect(group: .nameserverConnect, result: $0))
            }
        }
        group.addTask {
            await Probe.pingEach(Probe.defaultGatewayIps()) {
                continuation.yield(.ping(group: .gatewayReachability, result: $0))
            }
        }
        group.addTask { continuation.yield(.proxy(Probe.proxyConfig())) }
        group.addTask { await continuation.yield(.path(Probe.pathStatus())) }
        group.addTask { continuation.yield(.wifiRadio(Probe.wifiRadio())) }
        group.addTask { await continuation.yield(.wifiIdentity(Probe.wifiIdentity())) }
    }
}

extension Probe {
    static func pingEach(_ targets: [String], _ emit: @Sendable @escaping (PingResult) -> Void) async {
        await withTaskGroup(of: PingResult.self) { group in
            for target in targets {
                group.addTask { await ping(target) }
            }
            for await result in group {
                emit(result)
            }
        }
    }

    static func connectEach(
        _ targets: [String], port: UInt16, _ emit: @Sendable @escaping (ConnectResult) -> Void
    ) async {
        await withTaskGroup(of: ConnectResult.self) { group in
            for target in targets {
                group.addTask { await connect(target, port: port) }
            }
            for await result in group {
                emit(result)
            }
        }
    }

    static func resolveEach(
        _ domains: [String], _ emit: @Sendable @escaping (ResolutionResult) -> Void
    ) async {
        await withTaskGroup(of: ResolutionResult.self) { group in
            for domain in domains {
                group.addTask { await resolve(domain) }
            }
            for await result in group {
                emit(result)
            }
        }
    }
}
