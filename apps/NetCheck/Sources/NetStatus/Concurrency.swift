import Foundation

/// A one-shot latch shared by the callback-style network APIs: `claim()`
/// returns true exactly once, so a completion path that a delegate may
/// fire more than once resumes its continuation only on the first call.
final class OnceFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var claimed = false

    func claim() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if claimed {
            return false
        }
        claimed = true
        return true
    }
}
