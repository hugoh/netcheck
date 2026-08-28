import Foundation

/// Runs a blocking piece of work off the Swift concurrency cooperative
/// pool. The probes wrap POSIX calls (`getaddrinfo`, `connect`, raw ICMP)
/// that block their thread; without this a `TaskGroup` fan-out of them
/// would starve the pool.
func blocking<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
    await withCheckedContinuation { continuation in
        DispatchQueue.global().async { continuation.resume(returning: work()) }
    }
}

/// Milliseconds elapsed since `start`.
func millisecondsSince(_ start: Date) -> Double {
    -start.timeIntervalSinceNow * 1000.0
}
