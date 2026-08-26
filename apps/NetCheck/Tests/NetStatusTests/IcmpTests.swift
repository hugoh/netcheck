import Foundation
@testable import NetStatus
import Testing

struct IcmpTests {
    @Test func unparsableTargetIsUnreachable() async {
        let result = await Probe.ping("not-an-ip")
        #expect(!result.reachable)
        #expect(result.rttMs == nil)
    }

    @Test func mapsEchoRttToReachable() async {
        let result = await Probe.ping("192.0.2.1", echo: { _ in 0.05 })
        #expect(result.reachable)
        #expect(result.rttMs == 50)
    }

    @Test func mapsNilEchoToUnreachable() async {
        let result = await Probe.ping("192.0.2.1", echo: { _ in nil })
        #expect(!result.reachable)
        #expect(result.rttMs == nil)
    }
}

@Suite(.live, .tags(.network), .timeLimit(.minutes(1)))
struct IcmpLiveTests {
    @Test func pingsLoopback() async {
        let result = await Probe.ping("127.0.0.1")
        #expect(result.reachable)
        #expect(result.rttMs != nil)
        #expect((result.rttMs ?? 0) >= 0)
    }

    @Test func pingsLoopbackV6() async {
        let result = await Probe.ping("::1")
        #expect(result.reachable)
        #expect(result.rttMs != nil)
    }
}
