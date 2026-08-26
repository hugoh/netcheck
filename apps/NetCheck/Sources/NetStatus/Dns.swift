import Darwin
import Foundation
import SystemConfiguration

public extension Probe {
    /// Every network service's DNS configuration, each scoped to its own
    /// interface; the service backing the default route (`PrimaryService`)
    /// is additionally emitted as the unscoped systemwide resolver. A
    /// from-scratch `SCDynamicStore` derivation, not a reproduction of
    /// `scutil --dns` (no mDNS/private-SPI entries).
    static func listResolvers() -> [Resolver] {
        guard let store = SCStore.make("netcheck-dns") else { return [] }

        let primaryService = SCStore.dictionary(store, "State:/Network/Global/IPv4")
            .flatMap { SCStore.string($0, "PrimaryService") }

        var resolvers: [Resolver] = []
        for key in SCStore.keys(store, matching: "State:/Network/Service/.*/DNS") {
            guard let dnsDict = SCStore.dictionary(store, key) else { continue }

            let nameservers = SCStore.stringArray(dnsDict, "ServerAddresses")
            let ifName = interfaceName(store, forServiceKey: key)
            let ifIndex = ifName.flatMap(interfaceIndex)
            let reachable = nameservers.first.map(SCStore.isReachable) ?? false
            let isPrimary = primaryService.map { key.contains("/Service/\($0)/") } ?? false

            resolvers.append(
                buildResolver(dnsDict, ifName: ifName, ifIndex: ifIndex, scoped: true, reachable: reachable)
            )
            if isPrimary {
                resolvers.append(
                    buildResolver(dnsDict, ifName: nil, ifIndex: nil, scoped: false, reachable: reachable)
                )
            }
        }
        return resolvers
    }

    internal static func buildResolver(
        _ dnsDict: [String: Any], ifName: String?, ifIndex: UInt32?, scoped: Bool, reachable: Bool
    ) -> Resolver {
        Resolver(
            domain: SCStore.string(dnsDict, "DomainName"),
            searchDomains: SCStore.stringArray(dnsDict, "SearchDomains"),
            nameservers: SCStore.stringArray(dnsDict, "ServerAddresses"),
            ifIndex: ifIndex,
            ifName: ifName,
            scoped: scoped,
            reachable: reachable
        )
    }

    /// True when any scoped resolver is bound to a tunnel interface — i.e.
    /// split-DNS via a VPN.
    static func hasSplitDns(_ resolvers: [Resolver]) -> Bool {
        resolvers.contains { $0.scoped && ($0.ifName.map(isTunnelInterface) ?? false) }
    }

    /// Unique nameserver IPs across all resolvers, first-seen order.
    static func nameserverIps(_ resolvers: [Resolver]) -> [String] {
        var ips: [String] = []
        for resolver in resolvers {
            for nameserver in resolver.nameservers where !ips.contains(nameserver) {
                ips.append(nameserver)
            }
        }
        return ips
    }

    /// Domains only resolvable through a VPN tunnel's own resolver — each
    /// tunnel-bound scoped resolver's domain (or first search domain),
    /// deduplicated, order preserved.
    static func vpnScopedDomains(_ resolvers: [Resolver]) -> [String] {
        var domains: [String] = []
        for resolver in resolvers {
            guard resolver.scoped, resolver.ifName.map(isTunnelInterface) ?? false else { continue }
            guard let domain = resolver.domain ?? resolver.searchDomains.first,
                  !domains.contains(domain)
            else { continue }
            domains.append(domain)
        }
        return domains
    }

    private static func interfaceName(
        _ store: SCDynamicStore, forServiceKey dnsKey: String
    ) -> String? {
        for family in ["IPv4", "IPv6"] {
            let key = dnsKey.replacingOccurrences(of: "/DNS", with: "/\(family)")
            if let dict = SCStore.dictionary(store, key),
               let name = SCStore.string(dict, "InterfaceName") {
                return name
            }
        }
        return nil
    }

    private static func interfaceIndex(_ name: String) -> UInt32? {
        let index = if_nametoindex(name)
        return index != 0 ? index : nil
    }
}
