@testable import NetStatus
import Testing

/// The connectivity rollup is pure — lock it down so a probe change can't
/// silently shift it.
struct ConnectivityTests {
    @Test func majorityOfEmptyIsNeverOk() {
        #expect(NetworkStatus.majorityOk([Int]()) { _ in true } == false)
    }

    @Test func majorityNeedsStrictlyMoreThanHalf() {
        #expect(NetworkStatus.majorityOk([true, true, false]) { $0 })
        #expect(NetworkStatus.majorityOk([true, false]) { $0 } == false)
    }

    @Test func threeOrTwoCategoriesIsOnline() {
        #expect(NetworkStatus.classify(dnsOk: true, pingOk: true, tcpOk: true) == .online)
        #expect(NetworkStatus.classify(dnsOk: true, pingOk: false, tcpOk: true) == .online)
    }

    @Test func oneCategoryIsLimitedZeroIsOffline() {
        #expect(NetworkStatus.classify(dnsOk: true, pingOk: false, tcpOk: false) == .limited)
        #expect(NetworkStatus.classify(dnsOk: false, pingOk: false, tcpOk: false) == .offline)
    }

    @Test func detectedCaptivePortalCapsOnlineToLimited() {
        let off = ProxyEndpoint(enabled: false, host: nil, port: nil)
        let status = NetworkStatus(
            interfaces: [],
            vpn: .init(
                tunnels: [], primaryInterface: nil, connected: false,
                splitTunnel: false, routedSubnets: []
            ),
            resolvers: [], splitDns: false,
            reachability: [PingResult(target: "1.1.1.1", reachable: true, rttMs: 1)],
            reachabilityV6: [],
            resolution: [
                ResolutionResult(domain: "a", resolved: true, addresses: ["1"], durationMs: 1),
            ],
            domainReachability: [ConnectResult(target: "a", port: 443, reachable: true, rttMs: 1)],
            gatewayReachability: [], nameserverReachability: [], nameserverConnect: [],
            proxy: .init(http: off, https: off, socks: off, pacUrl: nil, exceptions: []),
            wifi: WifiStatus(
                identity: .init(connected: false, ssid: nil),
                radio: .init(
                    channel: nil, signalDbm: nil, noiseDbm: nil, security: nil, phyMode: nil
                )
            ),
            ipStack: .none,
            path: .init(
                status: .satisfied, primaryInterface: .wifi, expensive: false,
                constrained: false, supportsIPv4: true, supportsIPv6: false
            ),
            captivePortal: .detected
        )
        #expect(status.connectivity == .limited)
    }

    // MARK: degraded tier

    private static func pings(_ rtts: [Double?]) -> [PingResult] {
        rtts.enumerated().map { PingResult(target: "p\($0.offset)", reachable: $0.element != nil, rttMs: $0.element) }
    }

    private static func connects(_ rtts: [Double?]) -> [ConnectResult] {
        rtts.enumerated().map {
            ConnectResult(target: "c\($0.offset)", port: 443, reachable: $0.element != nil, rttMs: $0.element)
        }
    }

    private static func resolves(_ ms: [Double]) -> [ResolutionResult] {
        ms.enumerated().map {
            ResolutionResult(domain: "d\($0.offset)", resolved: true, addresses: ["x"], durationMs: $0.element)
        }
    }

    private static func rollup(
        dns: [Double] = [20, 20, 20], v4: [Double?] = [10, 10, 10], v6: [Double?] = [],
        tcp: [Double?] = [30, 30, 30], portal: CaptivePortalStatus = .clear, ipStack: IpStack? = nil
    ) -> Connectivity {
        NetworkStatus.connectivity(
            resolution: resolves(dns), reachability: pings(v4), reachabilityV6: pings(v6),
            domainReachability: connects(tcp), captivePortal: portal, ipStack: ipStack
        )
    }

    @Test func fastHealthyProbesAreOnline() {
        #expect(Self.rollup() == .online)
    }

    @Test func slowTcpMedianIsDegraded() {
        #expect(Self.rollup(tcp: [1000, 1500, 1800]) == .degraded)
        #expect(Self.rollup(tcp: [100, 200, 1800]) == .online)
    }

    @Test func slowDnsMedianIsDegraded() {
        #expect(Self.rollup(dns: [600, 700, 800]) == .degraded)
    }

    @Test func pingLossOverQuarterIsDegraded() {
        #expect(Self.rollup(v4: [10, 10, 10, nil]) == .online) // 25% is allowed
        #expect(Self.rollup(v4: [10, 10, 10, 10, 10, 10, nil, nil]) == .online) // 2/8
        #expect(Self.rollup(v4: [10, 10, 10, 10, 10, nil, nil]) == .degraded) // 2/7
        #expect(Self.rollup(v4: [10, 10, 10, 10, nil, nil, nil]) == .degraded) // 3/7
    }

    @Test func v6OnlyHealthyIsNotPenalisedForDeadV4() {
        #expect(Self.rollup(v4: [nil, nil, nil], v6: [10, 10, 10]) == .online)
    }

    @Test func deadV6OnDualStackIsDegraded() {
        #expect(Self.rollup(v6: [nil, nil, nil], ipStack: .dualStack) == .degraded)
        #expect(Self.rollup(v6: [nil, nil, nil], ipStack: .ipv4Only) == .online)
        #expect(Self.rollup(v6: [nil, nil, nil], ipStack: nil) == .online)
    }

    @Test func degradedNeverMasksWorseTiers() {
        #expect(Self.rollup(v4: [nil, nil, nil], tcp: [nil, nil, nil]) == .limited)
        #expect(Self.rollup(tcp: [2000, 2000, 2000], portal: .detected) == .limited)
    }

    @Test func slowSampleFromTheFieldIsDegraded() {
        let v4: [Double?] = [1564.5, 664.9, nil, nil, 1564.5, 1766.6, nil]
        let tcp: [Double?] = [1858.9, 1581.9, 1566.4, 1566.7, 1603.3, 1767.7, 1033.1]
        #expect(Self.rollup(v4: v4, tcp: tcp) == .degraded)
    }
}
