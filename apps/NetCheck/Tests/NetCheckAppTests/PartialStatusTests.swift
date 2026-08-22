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

    private func envelope(splitDns: Bool?) -> StatusFieldEnvelope {
        let json: [String: Any] = splitDns.map { ["SplitDns": $0] } ?? [:]
        let data = try! JSONSerialization.data(withJSONObject: json)
        return try! JSONDecoder().decode(StatusFieldEnvelope.self, from: data)
    }
}
