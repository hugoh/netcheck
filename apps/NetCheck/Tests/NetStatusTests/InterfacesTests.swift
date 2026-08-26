@testable import NetStatus
import Testing

private func iface(
    _ name: String, up: Bool, loopback: Bool = false, _ addresses: [String]
) -> NetInterface {
    NetInterface(name: name, up: up, loopback: loopback, addresses: addresses)
}

struct InterfaceClassificationTests {
    @Test func downInterfaceIsDownEvenWithAddress() {
        #expect(iface("en1", up: false, ["192.168.1.5/24"]).classification == .down)
    }

    @Test func upWithNoAddressesIsUnaddressed() {
        #expect(iface("en1", up: true, []).classification == .unaddressed)
    }

    @Test func upWithOnlyLinkLocalIsLinkLocalOnly() {
        #expect(iface("en1", up: true, ["fe80::1/64", "169.254.1.2/16"]).classification == .linkLocalOnly)
    }

    @Test func upWithAnyRoutableIsRoutable() {
        #expect(iface("en0", up: true, ["fe80::1/64", "192.168.1.5/24"]).classification == .routable)
    }

    @Test func classOrdersOnlineToOffline() {
        #expect(InterfaceClass.routable < .linkLocalOnly)
        #expect(InterfaceClass.linkLocalOnly < .unaddressed)
        #expect(InterfaceClass.unaddressed < .down)
    }

    @Test func linkLocalDetection() {
        #expect(isLinkLocal("169.254.1.2/16"))
        #expect(isLinkLocal("fe80::1%en0"))
        #expect(isLinkLocal("FE80::1/64"))
        #expect(!isLinkLocal("192.168.1.1/24"))
        #expect(!isLinkLocal("2001:db8::1/64"))
    }
}

struct IpStackTests {
    @Test func dualStackWhenBothFamiliesPresent() {
        #expect(Probe.ipStack([iface("en0", up: true, ["192.168.1.42/24", "2001:db8::1/64"])]) == .dualStack)
    }

    @Test func ipv4OnlyWhenNoRoutableV6() {
        #expect(Probe.ipStack([iface("en0", up: true, ["192.168.1.42/24", "fe80::1/64"])]) == .ipv4Only)
    }

    @Test func ipv6OnlyWhenNoV4() {
        #expect(Probe.ipStack([iface("en0", up: true, ["2001:db8::1/64"])]) == .ipv6Only)
    }

    @Test func noneWhenNoRoutableAddress() {
        #expect(Probe.ipStack([iface("en0", up: true, ["fe80::1/64"])]) == .none)
    }

    @Test func ignoresDownAndLoopback() {
        let interfaces = [
            iface("lo0", up: true, loopback: true, ["127.0.0.1/8", "::1/128"]),
            iface("en1", up: false, ["10.0.0.5/24"]),
        ]
        #expect(Probe.ipStack(interfaces) == .none)
    }
}

struct ListInterfacesTests {
    /// Ungated: `getifaddrs` never fails on macOS. Asserts only that every
    /// entry has a name (tolerates an empty list) so it stays deterministic.
    @Test func everyInterfaceHasAName() {
        for iface in Probe.listInterfaces() {
            #expect(!iface.name.isEmpty)
        }
    }
}

@Suite(.live, .tags(.liveSystem), .timeLimit(.minutes(1)))
struct ListInterfacesLiveTests {
    @Test func reportsLoopbackFromTheRealSystem() {
        let interfaces = Probe.listInterfaces()
        let loopback = interfaces.first { $0.name == "lo0" }
        #expect(loopback != nil)
        #expect(loopback?.loopback == true)
        #expect(loopback?.up == true)
        #expect(loopback?.addresses.contains("127.0.0.1/8") == true)
    }

    @Test func ipv4AddressesSortBeforeIpv6() {
        for iface in Probe.listInterfaces() {
            let firstV6 = iface.addresses.firstIndex { $0.contains(":") }
            let lastV4 = iface.addresses.lastIndex { !$0.contains(":") }
            if let firstV6, let lastV4 {
                #expect(lastV4 < firstV6)
            }
        }
    }
}
