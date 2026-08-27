import Foundation
import Testing
@testable import NetCheckApp

struct VpnScopedDomainsTests {
    private func resolver(
        domain: String? = nil,
        searchDomains: [String] = [],
        ifName: String? = nil,
        scoped: Bool = true
    ) -> Resolver {
        Resolver(
            domain: domain,
            searchDomains: searchDomains,
            nameservers: ["10.0.0.1"],
            ifIndex: nil,
            ifName: ifName,
            scoped: scoped,
            reachable: true
        )
    }

    @Test func includesScopedUtunResolverDomain() {
        let resolvers = [resolver(domain: "corp.internal", ifName: "utun3", scoped: true)]
        #expect(vpnScopedDomains(resolvers) == ["corp.internal"])
    }

    @Test func fallsBackToFirstSearchDomainWhenDomainIsNil() {
        let resolvers = [resolver(searchDomains: ["corp.internal", "other"], ifName: "utun3", scoped: true)]
        #expect(vpnScopedDomains(resolvers) == ["corp.internal"])
    }

    @Test func excludesUnscopedResolvers() {
        let resolvers = [resolver(domain: "corp.internal", ifName: "utun3", scoped: false)]
        #expect(vpnScopedDomains(resolvers) == [])
    }

    @Test func excludesNonUtunInterfaces() {
        let resolvers = [resolver(domain: "corp.internal", ifName: "en0", scoped: true)]
        #expect(vpnScopedDomains(resolvers) == [])
    }

    @Test func excludesResolversWithNoDomainOrSearchDomain() {
        let resolvers = [resolver(ifName: "utun3", scoped: true)]
        #expect(vpnScopedDomains(resolvers) == [])
    }

    @Test func deduplicatesRepeatedDomains() {
        let resolvers = [
            resolver(domain: "corp.internal", ifName: "utun3", scoped: true),
            resolver(domain: "corp.internal", ifName: "utun4", scoped: true),
        ]
        #expect(vpnScopedDomains(resolvers) == ["corp.internal"])
    }
}

struct PartialNetworkStatusTests {
    @Test func hasAnyIsFalseWhenEmpty() {
        #expect(!PartialNetworkStatus().hasAny)
    }

    @Test func hasAnyIsTrueAfterMergingAField() {
        var status = PartialNetworkStatus()
        status.merge(envelope(splitDns: true))
        #expect(status.hasAny)
    }

    @Test func mergeSetsFieldFromEnvelope() {
        var status = PartialNetworkStatus()
        status.merge(envelope(splitDns: true))
        #expect(status.splitDns == true)
    }

    @Test func mergeLeavesUnrelatedFieldsUntouched() {
        var status = PartialNetworkStatus()
        status.merge(envelope(splitDns: true))
        #expect(status.vpn == nil)
    }

    @Test func mergeDoesNotBlankAPreviouslySetFieldToNil() {
        var status = PartialNetworkStatus()
        status.merge(envelope(splitDns: true))
        status.merge(envelope(splitDns: nil))
        #expect(status.splitDns == true)
    }

    @Test func mergeOverwritesWithNewValue() {
        var status = PartialNetworkStatus()
        status.merge(envelope(splitDns: true))
        status.merge(envelope(splitDns: false))
        #expect(status.splitDns == false)
    }

    @Test func mergeSetsCaptivePortalFromEnvelope() {
        var status = PartialNetworkStatus()
        let json: [String: Any] = ["CaptivePortal": "Detected"]
        let data = try! JSONSerialization.data(withJSONObject: json)
        let envelope = try! JSONDecoder().decode(StatusFieldEnvelope.self, from: data)
        status.merge(envelope)
        #expect(status.captivePortal == .detected)
    }

    private func envelope(splitDns: Bool?) -> StatusFieldEnvelope {
        let json: [String: Any] = splitDns.map { ["SplitDns": $0] } ?? [:]
        let data = try! JSONSerialization.data(withJSONObject: json)
        return try! JSONDecoder().decode(StatusFieldEnvelope.self, from: data)
    }

    private func decode(_ json: [String: Any]) -> StatusFieldEnvelope {
        let data = try! JSONSerialization.data(withJSONObject: json)
        return try! JSONDecoder().decode(StatusFieldEnvelope.self, from: data)
    }

    @Test func mergeUpsertsPingResultByTargetInsteadOfReplacingTheWholeList() {
        var status = PartialNetworkStatus()
        status.merge(decode([
            "Ping": ["group": "Reachability", "result": ["target": "1.1.1.1", "reachable": true, "rtt_ms": 12.0]],
        ]))
        status.merge(decode([
            "Ping": ["group": "Reachability", "result": ["target": "8.8.8.8", "reachable": true, "rtt_ms": 8.0]],
        ]))
        #expect(status.reachability.map(\.target) == ["1.1.1.1", "8.8.8.8"])

        // A later result for the same target replaces it in place rather
        // than appending a duplicate or reordering the list.
        status.merge(decode([
            "Ping": ["group": "Reachability", "result": ["target": "1.1.1.1", "reachable": false, "rtt_ms": NSNull()]],
        ]))
        #expect(status.reachability.map(\.target) == ["1.1.1.1", "8.8.8.8"])
        #expect(status.reachability.first?.reachable == false)
    }

    @Test func mergeIgnoresPingUpdatesForGroupsThisUIDoesNotSurface() {
        var status = PartialNetworkStatus()
        status.merge(decode([
            "Ping": ["group": "GatewayReachability", "result": ["target": "10.0.0.1", "reachable": true, "rtt_ms": 1.0]],
        ]))
        #expect(status.reachability.isEmpty)
        #expect(status.reachabilityV6.isEmpty)
    }

    @Test func mergeMarksGroupComplete() {
        var status = PartialNetworkStatus()
        #expect(status.completedGroups.isEmpty)
        status.merge(decode(["GroupComplete": "Reachability"]))
        #expect(status.completedGroups == [.reachability])
    }
}

struct ConnectionConfidenceTests {
    private func status(
        dnsOk: Bool,
        pingOk: Bool,
        tcpOk: Bool
    ) -> PartialNetworkStatus {
        var status = PartialNetworkStatus()
        status.resolution = [ResolutionResult(domain: "example.com", resolved: dnsOk, addresses: [], durationMs: nil)]
        status.reachability = [PingResult(target: "1.1.1.1", reachable: pingOk, rttMs: nil)]
        status.reachabilityV6 = [PingResult(target: "::1", reachable: false, rttMs: nil)]
        status.domainReachability = [ConnectResult(target: "example.com", port: 443, reachable: tcpOk, rttMs: nil)]
        status.completedGroups = [.resolution, .reachability, .reachabilityV6, .domainReachability]
        return status
    }

    @Test func nilWhenSignalsMissing() {
        #expect(PartialNetworkStatus().confidence == nil)
    }

    @Test func onlineWhenAllSignalsOk() {
        #expect(status(dnsOk: true, pingOk: true, tcpOk: true).confidence == .online)
    }

    @Test func onlineWhenTwoSignalsOk() {
        #expect(status(dnsOk: true, pingOk: true, tcpOk: false).confidence == .online)
        #expect(status(dnsOk: true, pingOk: false, tcpOk: true).confidence == .online)
        #expect(status(dnsOk: false, pingOk: true, tcpOk: true).confidence == .online)
    }

    @Test func limitedWhenOneSignalOk() {
        #expect(status(dnsOk: true, pingOk: false, tcpOk: false).confidence == .limited)
    }

    @Test func offlineWhenNoSignalsOk() {
        #expect(status(dnsOk: false, pingOk: false, tcpOk: false).confidence == .offline)
    }

    @Test func pingOkCountsV6OnlyReachability() {
        var status = PartialNetworkStatus()
        status.resolution = [ResolutionResult(domain: "example.com", resolved: false, addresses: [], durationMs: nil)]
        status.reachability = [PingResult(target: "1.1.1.1", reachable: false, rttMs: nil)]
        status.reachabilityV6 = [PingResult(target: "::1", reachable: true, rttMs: nil)]
        status.domainReachability = [ConnectResult(target: "example.com", port: 443, reachable: false, rttMs: nil)]
        status.completedGroups = [.resolution, .reachability, .reachabilityV6, .domainReachability]
        #expect(status.confidence == .limited)
    }

    @Test func confidenceWaitsForEachGroupToComplete() {
        var status = PartialNetworkStatus()
        status.resolution = [ResolutionResult(domain: "example.com", resolved: true, addresses: [], durationMs: nil)]
        status.reachability = [PingResult(target: "1.1.1.1", reachable: true, rttMs: nil)]
        status.reachabilityV6 = [PingResult(target: "::1", reachable: true, rttMs: nil)]
        status.domainReachability = [ConnectResult(target: "example.com", port: 443, reachable: true, rttMs: nil)]
        #expect(status.confidence == nil, "no group has been marked complete yet")

        status.completedGroups.insert(.reachability)
        #expect(status.confidence == nil, "still missing reachabilityV6/resolution/domainReachability")

        status.completedGroups.formUnion([.reachabilityV6, .resolution, .domainReachability])
        #expect(status.confidence == .online)
    }
}
