import Foundation
import Network

/// Emits an event whenever the OS network path changes — interface
/// up/down, address/gateway reconfiguration, a VPN connecting or dropping —
/// via `NWPathMonitor`. Silent changes it can't see (a captive-portal
/// sign-in on an unchanged association, a resolver swap) are caught by the
/// caller's periodic full re-check.
public final class NetworkChangeMonitor: Sendable {
    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "net.larve.netcheck.pathmonitor")

    public init() {}

    /// A stream that yields once per path change. The first change the
    /// monitor reports after start is suppressed — it's the initial state,
    /// not a change — so a consumer that already ran one check at startup
    /// doesn't immediately run a second.
    public func changes() -> AsyncStream<Void> {
        AsyncStream { continuation in
            let sentInitial = OnceFlag()
            monitor.pathUpdateHandler = { _ in
                if sentInitial.claim() {
                    return
                }
                continuation.yield(())
            }
            monitor.start(queue: queue)
            continuation.onTermination = { [monitor] _ in monitor.cancel() }
        }
    }
}
