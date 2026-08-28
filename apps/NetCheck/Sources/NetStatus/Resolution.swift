import Darwin
import Foundation

extension Probe {
    /// Resolves a domain via `getaddrinfo`, timing the lookup.
    /// `resolved` is true only when at least one address
    /// comes back, and `durationMs` is set only then. Addresses are
    /// rendered bare (no port, no brackets) in `getaddrinfo` order.
    public static func resolve(_ domain: String) async -> ResolutionResult {
        await resolve(domain, lookup: lookupAddresses)
    }

    static func resolve(
        _ domain: String,
        lookup: @Sendable @escaping (String) -> [String]
    ) async -> ResolutionResult {
        await blocking {
            let start = Date()
            let addresses = lookup(domain)
            guard !addresses.isEmpty else {
                return ResolutionResult(domain: domain, resolved: false, addresses: [], durationMs: nil)
            }
            return ResolutionResult(
                domain: domain, resolved: true, addresses: addresses,
                durationMs: millisecondsSince(start)
            )
        }
    }

    static func lookupAddresses(_ host: String) -> [String] {
        var hints = addrinfo()
        hints.ai_socktype = SOCK_STREAM

        var result: UnsafeMutablePointer<addrinfo>?
        guard getaddrinfo(host, nil, &hints, &result) == 0 else { return [] }
        defer { freeaddrinfo(result) }

        var addresses: [String] = []
        var node = result
        while let current = node {
            defer { node = current.pointee.ai_next }
            if let address = Self.ipString(current.pointee.ai_addr) {
                addresses.append(address)
            }
        }
        return addresses
    }
}
