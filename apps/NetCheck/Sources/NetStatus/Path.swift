import Foundation
import Network

public extension Probe {
    /// A one-shot `NWPath` snapshot: start a monitor, take the first path it
    /// reports, stop it. Independent of the address/route probes — this is
    /// the OS's own view of the primary route.
    static func pathStatus() async -> PathStatus {
        let monitor = NWPathMonitor()
        let queue = DispatchQueue(label: "net.larve.netcheck.path")

        let path: NWPath = await withCheckedContinuation { continuation in
            let once = OnceFlag()
            monitor.pathUpdateHandler = { path in
                if once.claim() {
                    continuation.resume(returning: path)
                }
            }
            monitor.start(queue: queue)
        }
        monitor.cancel()

        return PathStatus(
            status: pathState(path.status),
            primaryInterface: pathInterfaceType(path.availableInterfaces.first?.type),
            expensive: path.isExpensive,
            constrained: path.isConstrained,
            supportsIPv4: path.supportsIPv4,
            supportsIPv6: path.supportsIPv6
        )
    }
}

func pathState(_ status: NWPath.Status) -> PathState {
    switch status {
    case .satisfied: .satisfied
    case .unsatisfied: .unsatisfied
    case .requiresConnection: .requiresConnection
    @unknown default: .unsatisfied
    }
}

func pathInterfaceType(_ type: NWInterface.InterfaceType?) -> PathInterfaceType {
    switch type {
    case .wifi: .wifi
    case .wiredEthernet: .wiredEthernet
    case .cellular: .cellular
    case .loopback: .loopback
    case .other: .other
    case nil: .none
    @unknown default: .other
    }
}
