import Foundation

/// Namespace for the individual network probes. Each probe lives in its
/// own file (`Interfaces`, `Dns`, `Vpn`, `Proxy`, `Wifi`, `Gateway`,
/// `CaptivePortal`, `Icmp`, `Resolution`, `Connect`); the `*All` helpers
/// here fan a probe out over a target list, preserving input order.
public enum Probe {
    static func pingAll(_ targets: [String]) async -> [PingResult] {
        await orderedResults(targets) { await ping($0) }
    }

    static func resolveAll(_ domains: [String]) async -> [ResolutionResult] {
        await orderedResults(domains) { await resolve($0) }
    }

    static func connectAll(_ targets: [String], port: UInt16) async -> [ConnectResult] {
        await orderedResults(targets) { await connect($0, port: port) }
    }

    private static func orderedResults<Result: Sendable>(
        _ items: [String], _ probe: @Sendable @escaping (String) async -> Result
    ) async -> [Result] {
        await withTaskGroup(of: (Int, Result).self) { group in
            for (index, item) in items.enumerated() {
                group.addTask { await (index, probe(item)) }
            }
            var results = [Result?](repeating: nil, count: items.count)
            for await (index, result) in group {
                results[index] = result
            }
            return results.compactMap(\.self)
        }
    }
}
