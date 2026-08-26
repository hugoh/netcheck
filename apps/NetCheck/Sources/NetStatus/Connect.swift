import Foundation
import Network

extension Probe {
    private static let connectTimeout: Duration = .seconds(2)

    /// Attempts a TCP handshake to `target:port` within 2s, timing it.
    /// `reachable` means `NWConnection` reached `.ready` (handshake
    /// complete); `rttMs` is set only then. A `.waiting` connection — no
    /// route, name resolution failing — counts as unreachable rather than
    /// being retried, since this is a one-shot probe.
    public static func connect(_ target: String, port: UInt16) async -> ConnectResult {
        await connect(target, port: port) { host, rawPort in
            await tcpHandshake(host: host, port: rawPort)
        }
    }

    static func connect(
        _ target: String,
        port: UInt16,
        handshake: @Sendable (String, UInt16) async -> Bool
    ) async -> ConnectResult {
        guard port != 0 else {
            return ConnectResult(target: target, port: port, reachable: false, rttMs: nil)
        }
        let start = Date()
        let reachable = await handshake(target, port)
        return ConnectResult(
            target: target, port: port, reachable: reachable,
            rttMs: reachable ? millisecondsSince(start) : nil
        )
    }

    private static func tcpHandshake(host: String, port rawPort: UInt16) async -> Bool {
        guard let port = NWEndpoint.Port(rawValue: rawPort) else { return false }
        let connection = NWConnection(host: NWEndpoint.Host(host), port: port, using: .tcp)
        let queue = DispatchQueue(label: "net.larve.netcheck.connect")

        return await withTaskGroup(of: Bool.self) { group in
            group.addTask {
                await withCheckedContinuation { (continuation: CheckedContinuation<Bool, Never>) in
                    let once = OnceFlag()
                    connection.stateUpdateHandler = { state in
                        switch state {
                        case .ready:
                            if once.claim() {
                                continuation.resume(returning: true)
                            }
                        case .waiting, .failed, .cancelled:
                            if once.claim() {
                                continuation.resume(returning: false)
                            }
                        default:
                            break
                        }
                    }
                    connection.start(queue: queue)
                }
            }
            group.addTask {
                try? await Task.sleep(for: connectTimeout)
                return false
            }
            let first = await group.next() ?? false
            // Drives the state handler to `.cancelled` so its continuation
            // resumes and the group can drain.
            connection.cancel()
            group.cancelAll()
            return first
        }
    }
}
