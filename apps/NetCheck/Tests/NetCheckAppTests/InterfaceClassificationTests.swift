import Testing
@testable import NetCheckApp

struct InterfaceClassificationTests {
    @Test func isLinkLocalAddressAcceptsIpv6LinkLocal() {
        #expect(isLinkLocalAddress("fe80::1"))
    }

    @Test func isLinkLocalAddressAcceptsIpv6LinkLocalUppercase() {
        #expect(isLinkLocalAddress("FE80::1"))
    }

    @Test func isLinkLocalAddressAcceptsIpv4LinkLocal() {
        #expect(isLinkLocalAddress("169.254.1.2"))
    }

    @Test func isLinkLocalAddressRejectsRoutableAddress() {
        #expect(!isLinkLocalAddress("192.168.1.2"))
    }

    @Test func isLinkLocalAddressStripsPrefixLength() {
        #expect(isLinkLocalAddress("fe80::1/64"))
        #expect(!isLinkLocalAddress("192.168.1.2/24"))
    }

    @Test func classifyDownWhenInterfaceIsNotUp() {
        let iface = NetInterface(name: "en0", up: false, loopback: false, addresses: ["192.168.1.2"])
        #expect(classify(iface) == .down)
    }

    @Test func classifyUnaddressedWhenUpWithNoAddresses() {
        let iface = NetInterface(name: "en0", up: true, loopback: false, addresses: [])
        #expect(classify(iface) == .unaddressed)
    }

    @Test func classifyLinkLocalOnlyWhenAllAddressesAreLinkLocal() {
        let iface = NetInterface(name: "en0", up: true, loopback: false, addresses: ["fe80::1", "169.254.1.2"])
        #expect(classify(iface) == .linkLocalOnly)
    }

    @Test func classifyRoutableWhenAnyAddressIsRoutable() {
        let iface = NetInterface(name: "en0", up: true, loopback: false, addresses: ["fe80::1", "192.168.1.2"])
        #expect(classify(iface) == .routable)
    }

    @Test func interfaceClassOrdersOnlineBeforeOffline() {
        #expect(InterfaceClass.routable < InterfaceClass.linkLocalOnly)
        #expect(InterfaceClass.linkLocalOnly < InterfaceClass.unaddressed)
        #expect(InterfaceClass.unaddressed < InterfaceClass.down)
    }
}
