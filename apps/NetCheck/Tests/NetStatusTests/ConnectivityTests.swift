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
}
