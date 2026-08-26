@testable import NetStatus
import Testing

private func iface(_ name: String, up: Bool, _ addresses: [String]) -> NetInterface {
    NetInterface(name: name, up: up, loopback: false, addresses: addresses)
}

private func resolver(
    domain: String? = nil, search: [String] = [], nameservers: [String],
    ifName: String?, scoped: Bool = true
) -> Resolver {
    Resolver(
        domain: domain, searchDomains: search, nameservers: nameservers,
        ifIndex: ifName.map { _ in 11 }, ifName: ifName, scoped: scoped, reachable: true
    )
}

struct VpnClassificationTests {
    @Test func noVpnWhenTunnelsHaveNoRoutableAddress() {
        let status = Probe.classifyTunnels(
            [iface("en0", up: true, ["192.168.1.42/24"]), iface("utun0", up: true, ["fe80::1/64"])],
            primary: "en0", routedSubnets: []
        )
        #expect(!status.connected)
        #expect(status.tunnels.isEmpty)
    }

    @Test func splitTunnelWhenPrimaryIsPhysical() {
        let status = Probe.classifyTunnels(
            [iface("en0", up: true, ["192.168.1.42/24"]), iface("utun3", up: true, ["10.10.0.5/24"])],
            primary: "en0", routedSubnets: []
        )
        #expect(status.connected)
        #expect(status.splitTunnel)
        #expect(status.tunnels == ["utun3"])
    }

    @Test func fullTunnelWhenPrimaryIsTheTunnel() {
        let status = Probe.classifyTunnels(
            [iface("utun3", up: true, ["10.10.0.5/24"])], primary: "utun3", routedSubnets: []
        )
        #expect(status.connected)
        #expect(!status.splitTunnel)
    }

    @Test func routedSubnetsClearedWhenNotConnected() {
        let status = Probe.classifyTunnels(
            [iface("en0", up: true, ["192.168.1.42/24"])], primary: "en0",
            routedSubnets: ["10.223.36.41/8"]
        )
        #expect(status.routedSubnets.isEmpty)
    }

    @Test func cidrAndOwnSubnetRoutes() {
        #expect(Probe.cidr("10.223.36.41", mask: "255.0.0.0") == "10.223.36.41/8")
        #expect(Probe.cidr("192.168.68.67", mask: "255.255.255.255") == "192.168.68.67/32")
        #expect(Probe.cidr("10.0.0.1", mask: "not-an-ip") == "10.0.0.1")
        #expect(
            Probe.ownSubnetRoutes(addresses: ["10.0.0.1", "10.0.0.2"], subnetMasks: ["255.0.0.0"])
                == ["10.0.0.1/8", "10.0.0.2"]
        )
    }
}

struct DnsHelperTests {
    private var en0: Resolver {
        resolver(nameservers: ["9.9.9.9"], ifName: "en0")
    }

    private var utun3: Resolver {
        resolver(domain: "corp.example.com", nameservers: ["10.10.10.10"], ifName: "utun3")
    }

    @Test func splitDnsDetectedForTunnelBoundScopedResolver() {
        #expect(Probe.hasSplitDns([en0, utun3]))
        #expect(!Probe.hasSplitDns([en0]))
        #expect(
            Probe.hasSplitDns([
                en0, resolver(domain: "corp", nameservers: ["10.10.10.10"], ifName: "ppp0"),
            ])
        )
    }

    @Test func vpnScopedDomainsCollectsDomainThenSearchFallback() {
        let utun4 = resolver(search: ["vpn.example.net"], nameservers: ["192.0.2.31"], ifName: "utun4")
        #expect(Probe.vpnScopedDomains([en0, utun3, utun4]) == ["corp.example.com", "vpn.example.net"])
    }

    @Test func nameserverIpsDedupPreservingOrder() {
        let a = resolver(nameservers: ["9.9.9.9"], ifName: "en0")
        let b = resolver(nameservers: ["9.9.9.9", "1.1.1.1"], ifName: "en0")
        #expect(Probe.nameserverIps([a, b]) == ["9.9.9.9", "1.1.1.1"])
        #expect(Probe.nameserverIps([]).isEmpty)
    }
}

struct ProxyConfigTests {
    @Test func noProxyConfigured() {
        let config = Probe.proxyConfig(from: [:])
        #expect(!config.http.enabled && !config.https.enabled && !config.socks.enabled)
        #expect(config.pacUrl == nil)
        #expect(config.exceptions.isEmpty)
    }

    @Test func httpProxyWithHostAndPort() {
        let config = Probe.proxyConfig(from: [
            "HTTPEnable": 1, "HTTPPort": 8080, "HTTPProxy": "proxy.example.com",
        ])
        #expect(config.http.enabled)
        #expect(config.http.host == "proxy.example.com")
        #expect(config.http.port == 8080)
    }

    @Test func pacUrlOnlyWhenEnabled() {
        #expect(
            Probe.proxyConfig(from: [
                "ProxyAutoConfigEnable": 1, "ProxyAutoConfigURLString": "http://e/x.pac",
            ]).pacUrl == "http://e/x.pac"
        )
        #expect(
            Probe.proxyConfig(from: [
                "ProxyAutoConfigEnable": 0, "ProxyAutoConfigURLString": "http://e/x.pac",
            ]).pacUrl == nil
        )
    }

    @Test func exceptionsListParsed() {
        #expect(
            Probe.proxyConfig(from: ["ExceptionsList": ["*.local", "169.254/16"]]).exceptions
                == ["*.local", "169.254/16"]
        )
    }
}
