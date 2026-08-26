import Foundation

/// Full-snapshot assembly. Runs independent probes concurrently so
/// wall-clock is the slowest single probe, not their sum. For the app's
/// progressive UI, `collectStreaming` yields each field as it lands
/// instead.
public enum NetStatus {
    public static func collect() async throws -> NetworkStatus {
        let interfaces = Probe.listInterfaces()
        let resolvers = Probe.listResolvers()
        let nameserverIps = Probe.nameserverIps(resolvers)

        async let wifiIdentityTask = Probe.wifiIdentity()
        async let pathTask = Probe.pathStatus()
        async let captiveTask = Probe.checkCaptivePortal()
        async let resolutionTask = Probe.resolveAll(Defaults.resolutionTargets)
        async let reachabilityTask = Probe.pingAll(Defaults.pingTargets)
        async let reachabilityV6Task = Probe.pingAll(Defaults.pingTargetsV6)
        async let domainReachabilityTask = Probe.connectAll(Defaults.resolutionTargets, port: 443)
        async let nameserverReachabilityTask = Probe.pingAll(nameserverIps)
        async let nameserverConnectTask = Probe.connectAll(nameserverIps, port: 53)
        async let gatewayReachabilityTask = Probe.pingAll(Probe.defaultGatewayIps())

        return await NetworkStatus(
            interfaces: interfaces,
            vpn: Probe.vpnStatus(interfaces),
            resolvers: resolvers,
            splitDns: Probe.hasSplitDns(resolvers),
            reachability: reachabilityTask,
            reachabilityV6: reachabilityV6Task,
            resolution: resolutionTask,
            domainReachability: domainReachabilityTask,
            gatewayReachability: gatewayReachabilityTask,
            nameserverReachability: nameserverReachabilityTask,
            nameserverConnect: nameserverConnectTask,
            proxy: Probe.proxyConfig(),
            wifi: WifiStatus(identity: wifiIdentityTask, radio: Probe.wifiRadio()),
            ipStack: Probe.ipStack(interfaces),
            path: pathTask,
            captivePortal: captiveTask
        )
    }

    /// Connectivity-only fast path — the three signal categories without
    /// the captive-portal cap.
    public static func connectivityOnly() async -> Connectivity {
        async let resolution = Probe.resolveAll(Defaults.resolutionTargets)
        async let reachability = Probe.pingAll(Defaults.pingTargets)
        async let reachabilityV6 = Probe.pingAll(Defaults.pingTargetsV6)
        async let domainReachability = Probe.connectAll(Defaults.resolutionTargets, port: 443)

        let resolutionResults = await resolution
        let reachabilityResults = await reachability
        let reachabilityV6Results = await reachabilityV6
        let domainReachabilityResults = await domainReachability

        let dnsOk = NetworkStatus.majorityOk(resolutionResults) { $0.resolved }
        let pingOk = NetworkStatus.majorityOk(reachabilityResults) { $0.reachable }
            || NetworkStatus.majorityOk(reachabilityV6Results) { $0.reachable }
        let tcpOk = NetworkStatus.majorityOk(domainReachabilityResults) { $0.reachable }
        return NetworkStatus.classify(dnsOk: dnsOk, pingOk: pingOk, tcpOk: tcpOk)
    }
}
