import Foundation
import Testing
@testable import NetCheckApp

/// Regression test for a real deadlock: a manual refresh (`netcheck stream`)
/// spawned while auto-refresh's persistent `netcheck watch` process is
/// already running, both read via `FileHandle.bytes.lines`, would never
/// complete — that async-sequence API deadlocks its second concurrent
/// reader in the same process. Fixed by switching `StatusFetcher.streamLines`
/// to `FileHandle.readabilityHandler`. This spawns real `netcheck`
/// subprocesses and hits the real network (same as the Rust
/// `collect_streaming` tests), so it's skipped rather than failed when no
/// built binary is available (e.g. `swift test` run standalone without a
/// prior `cargo build`).
@MainActor
struct StatusFetcherRegressionTests {
    @Test(.timeLimit(.minutes(1)))
    func manualRefreshCompletesWhileAutoRefreshIsRunning() async throws {
        try #require(Self.netcheckBinaryExists, "no built netcheck binary found — run `cargo build` first")

        let fetcher = StatusFetcher()

        // Let auto-refresh's initial `watch` fire start streaming before
        // issuing a manual refresh alongside it — this is the exact
        // concurrent-subprocess condition that triggered the deadlock.
        try await Task.sleep(for: .seconds(1))

        fetcher.refresh()
        #expect(fetcher.isRefreshing)

        // Bounded by the test's own .timeLimit, not an unbounded loop: if
        // this regresses, isRefreshing never clears and the test times out
        // (fails) instead of hanging CI forever.
        while fetcher.isRefreshing {
            try await Task.sleep(for: .milliseconds(200))
        }
        #expect(!fetcher.isRefreshing)
        #expect(fetcher.errorMessage == nil)

        fetcher.toggleAutoRefresh() // cancels watchTask, which kills its subprocess
        // ...but that teardown is asynchronous (Task cancellation is
        // cooperative), and `swift test` doesn't wait around for it — give
        // it a moment so this doesn't orphan a `netcheck watch` process on
        // every test run.
        try await Task.sleep(for: .milliseconds(200))
    }

    /// Mirrors `StatusFetcher.locateBinary()`'s own candidate paths exactly
    /// (kept separate rather than calling into that `private` method, since
    /// this is only a test guard, not behavior under test) — it looks
    /// specifically for a binary named `netcheck` (the `tui`-feature
    /// build), not `netcheck-core`.
    private static var netcheckBinaryExists: Bool {
        let fm = FileManager.default
        let candidates = [
            "target/release/netcheck",
            "target/debug/netcheck",
            "../../target/release/netcheck",
            "../../target/debug/netcheck",
        ]
        return candidates.contains { fm.fileExists(atPath: $0) }
    }
}
