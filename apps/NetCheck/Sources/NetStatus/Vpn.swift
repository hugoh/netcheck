import Darwin
import Foundation
import SystemConfiguration

/// A tunnel-family interface name (VPN/PPP). Shared by VPN classification
/// and split-DNS detection so both agree on what a VPN interface is.
func isTunnelInterface(_ name: String) -> Bool {
    name.hasPrefix("utun") || name.hasPrefix("ppp") || name.hasPrefix("tun")
}

extension Probe {
    /// VPN / tunnel status derived from the interface list plus the
    /// dynamic store's primary-interface and per-service route state.
    public static func vpnStatus(_ interfaces: [NetInterface]) -> VpnStatus {
        guard let store = SCStore.make("netcheck-vpn") else {
            return classifyTunnels(interfaces, primary: nil, routedSubnets: [])
        }
        let primary = SCStore.dictionary(store, "State:/Network/Global/IPv4")
            .flatMap { SCStore.string($0, "PrimaryInterface") }

        let routedSubnets = interfaces
            .filter { $0.up && isTunnelInterface($0.name) && hasRoutableAddress($0) }
            .flatMap { routesForInterface(store, ifName: $0.name) }

        return classifyTunnels(interfaces, primary: primary, routedSubnets: routedSubnets)
    }

    static func classifyTunnels(
        _ interfaces: [NetInterface], primary: String?, routedSubnets: [String]
    ) -> VpnStatus {
        let tunnels = interfaces
            .filter { $0.up && isTunnelInterface($0.name) && hasRoutableAddress($0) }
            .map(\.name)
        let connected = !tunnels.isEmpty
        let splitTunnel = connected && (primary.map { !isTunnelInterface($0) } ?? false)

        return VpnStatus(
            tunnels: tunnels,
            primaryInterface: primary,
            connected: connected,
            splitTunnel: splitTunnel,
            routedSubnets: connected ? routedSubnets : []
        )
    }

    private static func hasRoutableAddress(_ interface: NetInterface) -> Bool {
        interface.addresses.contains { !isLinkLocal($0) }
    }

    /// Routed subnets for `ifName`'s network service — its own
    /// `Addresses`/`SubnetMasks` plus each `AdditionalRoutes` entry.
    static func routesForInterface(_ store: SCDynamicStore, ifName: String) -> [String] {
        for key in SCStore.keys(store, matching: "State:/Network/Service/.*/IPv4") {
            guard let dict = SCStore.dictionary(store, key),
                  SCStore.string(dict, "InterfaceName") == ifName
            else { continue }

            var routes = ownSubnetRoutes(
                addresses: SCStore.stringArray(dict, "Addresses"),
                subnetMasks: SCStore.stringArray(dict, "SubnetMasks")
            )
            for route in SCStore.dictArray(dict, "AdditionalRoutes") {
                if let destination = SCStore.string(route, "DestinationAddress") {
                    routes.append(cidr(destination, mask: SCStore.string(route, "SubnetMask") ?? ""))
                }
            }
            return routes
        }
        return []
    }

    /// Pairs `Addresses` with `SubnetMasks` by position, keeping every
    /// address even if the mask array runs short.
    static func ownSubnetRoutes(addresses: [String], subnetMasks: [String]) -> [String] {
        addresses.enumerated().map { index, address in
            index < subnetMasks.count ? cidr(address, mask: subnetMasks[index]) : address
        }
    }

    static func cidr(_ address: String, mask: String) -> String {
        guard let prefix = prefixLength(dottedMask: mask) else { return address }
        return "\(address)/\(prefix)"
    }

    private static func prefixLength(dottedMask mask: String) -> Int? {
        var addr = in_addr()
        guard inet_pton(AF_INET, mask, &addr) == 1 else { return nil }
        return addr.s_addr.nonzeroBitCount
    }
}
