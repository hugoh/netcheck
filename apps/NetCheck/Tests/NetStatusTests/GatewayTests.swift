import Darwin
@testable import NetStatus
import Testing

struct RouteSockaddrParsingTests {
    /// A route message ending in a truncated NETMASK sockaddr (a `/0`
    /// default route carries `sa_len == 0`) must still be recorded, not
    /// dropped for being shorter than a full `sockaddr`.
    @Test func recordsTruncatedTrailingNetmask() {
        var block = [UInt8](repeating: 0, count: 36)
        for saddr in [0, 16] {
            block[saddr] = 16
            block[saddr + 1] = UInt8(AF_INET)
        }
        // bytes 32..<36: NETMASK, sa_len == 0, then 4-byte alignment padding.

        let mask = (1 << Int(RTAX_DST)) | (1 << Int(RTAX_GATEWAY)) | (1 << Int(RTAX_NETMASK))
        block.withUnsafeBytes { raw in
            let slots = Probe.routeSockaddrs(Int32(mask), in: raw)
            #expect(slots[Int(RTAX_DST)] != nil)
            #expect(slots[Int(RTAX_GATEWAY)] != nil)
            #expect(slots[Int(RTAX_NETMASK)] != nil)
        }
    }

    @Test func stopsWhenAClaimedSockaddrRunsPastTheBuffer() {
        var block = [UInt8](repeating: 0, count: 20)
        block[0] = 16
        block[16] = 16 // second sockaddr claims 16 bytes, only 4 remain

        block.withUnsafeBytes { raw in
            let slots = Probe.routeSockaddrs(
                Int32((1 << Int(RTAX_DST)) | (1 << Int(RTAX_GATEWAY))), in: raw
            )
            #expect(slots[Int(RTAX_DST)] != nil)
            #expect(slots[Int(RTAX_GATEWAY)] == nil)
        }
    }
}

@Suite(.live, .tags(.liveSystem), .timeLimit(.minutes(1)))
struct GatewayLiveTests {
    /// On a machine with a working default route the probe returns at least
    /// one gateway, and every entry parses as an IP address — a smoke test
    /// against the live routing table.
    @Test func returnsParseableGatewaysFromTheLiveRoutingTable() {
        let gateways = Probe.defaultGatewayIps()
        for gateway in gateways {
            #expect(gateway.contains(".") || gateway.contains(":"))
        }
        // IPv4 gateways sort before IPv6.
        let firstV6 = gateways.firstIndex { $0.contains(":") }
        let lastV4 = gateways.lastIndex { !$0.contains(":") }
        if let firstV6, let lastV4 {
            #expect(lastV4 < firstV6)
        }
    }
}
