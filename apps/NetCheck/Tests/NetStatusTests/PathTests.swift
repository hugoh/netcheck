import Foundation
@testable import NetStatus
import Network
import Testing

struct PathMappingTests {
    @Test func mapsStatus() {
        #expect(pathState(.satisfied) == .satisfied)
        #expect(pathState(.unsatisfied) == .unsatisfied)
        #expect(pathState(.requiresConnection) == .requiresConnection)
    }

    @Test func mapsInterfaceType() {
        #expect(pathInterfaceType(.wifi) == .wifi)
        #expect(pathInterfaceType(.wiredEthernet) == .wiredEthernet)
        #expect(pathInterfaceType(.cellular) == .cellular)
        #expect(pathInterfaceType(.loopback) == .loopback)
        #expect(pathInterfaceType(.other) == .other)
        #expect(pathInterfaceType(nil) == .none)
    }

    @Test func encodesEnumsAsSnakeCase() throws {
        let status = PathStatus(
            status: .requiresConnection, primaryInterface: .wiredEthernet,
            expensive: false, constrained: false, supportsIPv4: true, supportsIPv6: true
        )
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let object = try JSONSerialization.jsonObject(with: encoder.encode(status))
            as? [String: Any]
        #expect(object?["status"] as? String == "requires_connection")
        #expect(object?["primary_interface"] as? String == "wired_ethernet")
        #expect(object?["supports_ipv4"] as? Bool == true)
    }
}

@Suite(.live, .tags(.liveSystem), .timeLimit(.minutes(1)))
struct PathLiveTests {
    /// Smoke test against the live path snapshot: it returns without hanging
    /// and reports a self-consistent `PathStatus` — a satisfied path names a
    /// primary interface. Does not assume the machine is online.
    @Test func livePathSnapshotIsConsistent() async {
        let path = await Probe.pathStatus()
        if path.status == .satisfied {
            #expect(path.primaryInterface != .none)
        }
    }
}
