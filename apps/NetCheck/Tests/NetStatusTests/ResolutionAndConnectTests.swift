import Foundation
@testable import NetStatus
import Testing

struct ResolutionTests {
    @Test func mapsAddressesToResolvedWithDuration() async {
        let result = await Probe.resolve("example.test", lookup: { _ in ["1.2.3.4", "2606:4700::1"] })
        #expect(result.resolved)
        #expect(result.addresses == ["1.2.3.4", "2606:4700::1"])
        #expect((result.durationMs ?? -1) >= 0)
    }

    @Test func mapsEmptyLookupToUnresolved() async {
        let result = await Probe.resolve("example.test", lookup: { _ in [] })
        #expect(!result.resolved)
        #expect(result.addresses.isEmpty)
        #expect(result.durationMs == nil)
    }
}

@Suite(.live, .tags(.network), .timeLimit(.minutes(1)))
struct ResolutionLiveTests {
    @Test func resolvesAKnownDomain() async {
        let result = await Probe.resolve("cloudflare.com")
        #expect(result.resolved)
        #expect(!result.addresses.isEmpty)
        #expect((result.durationMs ?? 0) >= 0)
    }
}

struct ConnectTests {
    @Test func rejectsPortZeroWithoutAttemptingAHandshake() async {
        let result = await Probe.connect("example.test", port: 0, handshake: { _, _ in
            Issue.record("handshake should not be attempted for port 0")
            return true
        })
        #expect(!result.reachable)
        #expect(result.rttMs == nil)
    }

    @Test func mapsSuccessfulHandshakeToReachableRtt() async {
        let result = await Probe.connect("example.test", port: 443, handshake: { _, _ in true })
        #expect(result.reachable)
        #expect(result.rttMs != nil)
    }

    @Test func mapsFailedHandshakeToUnreachable() async {
        let result = await Probe.connect("192.0.2.1", port: 443, handshake: { _, _ in false })
        #expect(!result.reachable)
        #expect(result.rttMs == nil)
    }

    @Test func echoesRequestedPort() async {
        let result = await Probe.connect("example.test", port: 8443, handshake: { _, _ in false })
        #expect(result.port == 8443)
    }
}

@Suite(.live, .tags(.network), .timeLimit(.minutes(1)))
struct ConnectLiveTests {
    @Test func handshakeSucceedsToAKnownHost() async {
        let result = await Probe.connect("cloudflare.com", port: 443)
        #expect(result.reachable)
        #expect(result.rttMs != nil)
        #expect(result.port == 443)
    }
}
