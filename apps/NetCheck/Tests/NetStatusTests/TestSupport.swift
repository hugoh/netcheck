import Foundation
import Testing

extension Tag {
    /// Real DNS / TCP / ICMP / HTTP egress.
    @Tag static var network: Self
    /// Real system state: getifaddrs / sysctl route dump / NWPathMonitor.
    @Tag static var liveSystem: Self
}

extension Trait where Self == ConditionTrait {
    /// Gates tests that perform real network or system I/O. They run only
    /// when `NETCHECK_LIVE_TESTS=1` (CI sets it); a plain `swift test` stays
    /// deterministic and offline-safe.
    static var live: Self {
        .enabled(
            if: ProcessInfo.processInfo.environment["NETCHECK_LIVE_TESTS"] == "1",
            "set NETCHECK_LIVE_TESTS=1 to run live network/system tests"
        )
    }
}
