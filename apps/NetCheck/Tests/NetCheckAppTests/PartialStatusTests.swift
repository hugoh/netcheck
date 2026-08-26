@testable import NetCheckApp
import NetStatus
import Testing

struct PartialNetworkStatusTests {
    @Test func mergeFillsSingleValueFieldsAndNeverBlanksThem() {
        var status = PartialNetworkStatus()
        #expect(!status.hasAny)

        status.merge(.splitDns(true), generation: 1)
        #expect(status.splitDns == true)

        status.merge(.ipStack(.dualStack), generation: 1)
        #expect(status.ipStack == .dualStack)
        #expect(status.hasAny)
    }

    @Test func mergePathFillsPathAndCountsTowardHasAny() {
        var status = PartialNetworkStatus()
        status.merge(
            .path(
                .init(
                    status: .satisfied, primaryInterface: .wifi, expensive: true,
                    constrained: false, supportsIPv4: true, supportsIPv6: true
                )
            ),
            generation: 1
        )
        #expect(status.path?.primaryInterface == .wifi)
        #expect(status.path?.expensive == true)
        #expect(status.hasAny)
    }

    @Test func listFieldsUpsertByTargetKeepingPosition() {
        var status = PartialNetworkStatus()
        status.merge(
            .ping(group: .reachability, result: .init(target: "1.1.1.1", reachable: false, rttMs: nil)),
            generation: 1
        )
        status.merge(
            .ping(group: .reachability, result: .init(target: "8.8.8.8", reachable: true, rttMs: 5)),
            generation: 1
        )
        status.merge(
            .ping(group: .reachability, result: .init(target: "1.1.1.1", reachable: true, rttMs: 12)),
            generation: 2
        )

        #expect(status.reachability.map(\.target) == ["1.1.1.1", "8.8.8.8"])
        #expect(status.reachability[0].reachable)
        #expect(status.reachability[0].rttMs == 12)
    }

    @Test func isPendingTracksRowFreshnessAgainstGeneration() {
        var status = PartialNetworkStatus()
        status.merge(.resolution(.init(domain: "a", resolved: true, addresses: ["1"], durationMs: 1)), generation: 3)
        #expect(!status.isPending("resolution:a", asOf: 3))
        #expect(status.isPending("resolution:a", asOf: 4))
    }

    @Test func confidenceNilUntilAllGroupsCompleteAndCaptivePortalKnown() {
        var status = PartialNetworkStatus()
        for target in ["1.1.1.1", "8.8.8.8"] {
            status.merge(
                .ping(group: .reachability, result: .init(target: target, reachable: true, rttMs: 1)),
                generation: 1
            )
            status.merge(
                .connect(group: .domainReachability, result: .init(
                    target: target,
                    port: 443,
                    reachable: true,
                    rttMs: 1
                )),
                generation: 1
            )
            status.merge(
                .resolution(.init(domain: target, resolved: true, addresses: ["x"], durationMs: 1)),
                generation: 1
            )
        }
        status.merge(.groupComplete(.reachability), generation: 1)
        status.merge(.groupComplete(.reachabilityV6), generation: 1)
        status.merge(.groupComplete(.domainReachability), generation: 1)
        status.merge(.groupComplete(.resolution), generation: 1)
        #expect(status.confidence == nil) // captive portal still unknown

        status.merge(.captivePortal(.clear), generation: 1)
        #expect(status.confidence == .online)

        status.merge(.captivePortal(.detected), generation: 1)
        #expect(status.confidence == .limited)
    }

    @Test func isRefreshCompleteWaitsForTheFourConfidenceGroups() {
        var status = PartialNetworkStatus()
        status.merge(.groupComplete(.reachability), generation: 5)
        status.merge(.groupComplete(.reachabilityV6), generation: 5)
        status.merge(.groupComplete(.domainReachability), generation: 5)
        #expect(!status.isRefreshComplete(asOf: 5))
        status.merge(.groupComplete(.resolution), generation: 5)
        #expect(status.isRefreshComplete(asOf: 5))
    }
}
