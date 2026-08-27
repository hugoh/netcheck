import AppKit
import Foundation

/// Runs the `netcheck` CLI's `stream`/`watch` subcommands and decodes their
/// NDJSON output line by line as it arrives, publishing each field the
/// moment it's ready instead of waiting for a full snapshot — mirrors
/// netcheck-tui/-gui's streamed `PartialStatus` model. Reuses the
/// already-tested Rust `netstatus` core instead of re-implementing any
/// diagnostics (or refresh cadence) in Swift: auto-refresh runs `watch`,
/// which re-checks on OS network-config changes plus an adaptive-interval
/// poll as a safety net, instead of a fixed Swift-side timer.
/// Accumulates raw pipe reads and splits off complete lines. `@unchecked
/// Sendable`: a `readabilityHandler` is invoked serially by the OS for a
/// given `FileHandle` — never concurrently with itself — so a single
/// instance is safe to mutate across those calls despite not being
/// actor-isolated.
private final class LineBuffer: @unchecked Sendable {
    private var data = Data()

    func appendAndExtractLines(_ chunk: Data) -> [String] {
        data.append(chunk)
        var lines: [String] = []
        while let newline = data.firstIndex(of: UInt8(ascii: "\n")) {
            if let line = String(data: data[..<newline], encoding: .utf8) {
                lines.append(line)
            }
            data.removeSubrange(...newline)
        }
        return lines
    }
}

@MainActor
final class StatusFetcher: ObservableObject {
    @Published var status = PartialNetworkStatus()
    @Published var lastUpdated: Date?
    @Published var autoRefreshEnabled = true
    @Published var errorMessage: String?
    /// True for the duration of a manual (⌘R) refresh — visible proof the
    /// refresh actually ran, since two consecutive refreshes on a stable
    /// network can otherwise look identical (nothing else in the UI
    /// changes). Not raised for the background `watch` auto-refresh, which
    /// shouldn't visually interrupt the user for routine polling.
    @Published var isRefreshing = false

    /// Only used as the auto-refresh-off fallback — a one-shot `netcheck
    /// stream` process for a manual refresh, spawned when there's no
    /// persistent `watch` process to signal instead. `nil` whenever
    /// auto-refresh is on.
    private var refreshTask: Task<Void, Never>?
    /// Bumped on every manual refresh; used both to cancel a superseded
    /// fallback `refresh()` task and, via `PartialNetworkStatus.rowGeneration`
    /// / `groupCompleteGeneration`, to tell the view which
    /// reachability/resolution rows the in-progress refresh hasn't updated
    /// yet (`PartialNetworkStatus.isPending`) and when it's fully done
    /// (`PartialNetworkStatus.isRefreshComplete`).
    private(set) var refreshGeneration = 0
    private var watchTask: Task<Void, Never>?
    /// stdin of the currently-running `watch` process, if any — `refresh()`
    /// writes a `WatchCommand.refresh` line here instead of spawning a
    /// second concurrent subprocess when this is non-nil.
    private var watchStdin: FileHandle?
    private let binaryURL: URL?
    private var terminationObserver: NSObjectProtocol?

    init() {
        binaryURL = Self.locateBinary()
        // No separate manual `refresh()` here: `netcheck watch`'s own first
        // fire is already a full snapshot (mirrors the TUI, which dropped
        // its redundant initial-fetch thread for the same reason), so
        // starting it directly avoids running two independent full
        // collections at startup.
        startWatching()

        // Without this, quitting the app leaves its `netcheck watch`
        // subprocess running indefinitely — Process()-spawned children
        // aren't killed automatically when the parent exits on macOS, they
        // just get reparented to launchd. Cancelling the tasks here tears
        // down their subprocesses via streamLines' `continuation.onTermination`.
        // Done synchronously (via `assumeIsolated`, safe since `queue: .main`
        // guarantees this closure already runs on the main thread) rather
        // than spawning a new `Task` — the app process can exit right after
        // this notification returns, with no guarantee an async Task gets
        // scheduled before that happens.
        terminationObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.refreshTask?.cancel()
                self?.watchTask?.cancel()
            }
        }
    }

    func toggleAutoRefresh() {
        autoRefreshEnabled.toggle()
        if autoRefreshEnabled {
            startWatching()
        } else {
            watchTask?.cancel()
            watchTask = nil
            watchStdin = nil
        }
    }

    /// Starts the persistent `netcheck watch` subprocess and merges each
    /// NDJSON line it emits, for as long as auto-refresh stays on. Unlike
    /// the auto-refresh-off fallback in `refresh()`, this runs one
    /// long-lived process rather than restarting on a timer — the Rust
    /// side owns the config-change/poll cadence, and `refresh()` triggers
    /// an immediate check by writing to this same process's stdin instead
    /// of spawning a second one.
    private func startWatching() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }

        let (stream, stdin) = Self.streamLines(url: binaryURL, args: ["watch"])
        watchStdin = stdin
        watchTask = Task {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            do {
                for try await line in stream {
                    guard !line.isEmpty, let data = line.data(using: .utf8) else { continue }
                    guard let envelope = try? decoder.decode(StatusFieldEnvelope.self, from: data) else {
                        continue
                    }
                    status.merge(envelope, generation: refreshGeneration)
                    lastUpdated = Date()
                    errorMessage = nil
                    // A stdin-triggered refresh has no "process exited"
                    // completion signal the way the one-shot fallback does
                    // — watch for the same confidence-readiness signal
                    // `PartialNetworkStatus.confidence` itself waits on.
                    if isRefreshing, status.isRefreshComplete(asOf: refreshGeneration) {
                        isRefreshing = false
                    }
                }
            } catch is CancellationError {
                // Auto-refresh was toggled off; nothing to report.
            } catch {
                errorMessage = "Failed to run netcheck watch: \(error.localizedDescription)"
            }
            watchStdin = nil
        }
    }

    func refresh() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }

        refreshGeneration += 1
        isRefreshing = true

        if let watchStdin {
            // A `watch` process is already running — trigger it via stdin
            // instead of spawning a second concurrent subprocess (real
            // deadlock hazard, see streamLines' doc comment). Completion is
            // detected in startWatching()'s own merge loop.
            Self.send(.refresh, to: watchStdin)
            return
        }

        // Auto-refresh is off — no persistent process to signal, so fall
        // back to a one-shot `netcheck stream`, same as before this
        // process became bidirectional. Safe by construction: nothing else
        // reads a pipe concurrently while `watchTask`/`watchStdin` are nil.
        refreshTask?.cancel()
        let generation = refreshGeneration
        let (stream, _) = Self.streamLines(url: binaryURL, args: ["stream"])
        refreshTask = Task {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            do {
                for try await line in stream {
                    guard generation == refreshGeneration else { return }
                    guard !line.isEmpty, let data = line.data(using: .utf8) else { continue }
                    guard let envelope = try? decoder.decode(StatusFieldEnvelope.self, from: data) else {
                        continue
                    }
                    status.merge(envelope, generation: generation)
                    lastUpdated = Date()
                    errorMessage = nil
                }
            } catch is CancellationError {
                // Superseded by a newer refresh; nothing to report.
            } catch {
                guard generation == refreshGeneration else { return }
                errorMessage = "Failed to run netcheck: \(error.localizedDescription)"
            }
            if generation == refreshGeneration {
                isRefreshing = false
            }
        }
    }

    /// Runs `netcheck stream`/`watch`, yielding each line of its stdout as
    /// it's written (no waiting for the process to exit, no blocking reads)
    /// and returning a handle to write `WatchCommand` lines to its stdin —
    /// only meaningful for `watch`, which reads and acts on them; `stream`
    /// ignores stdin entirely, but there's no harm giving it a pipe nobody
    /// writes to.
    ///
    /// Uses `FileHandle.readabilityHandler` + manual line-buffering rather
    /// than the more modern `FileHandle.bytes.lines` async sequence: with
    /// two of these running concurrently in the same process — the
    /// long-lived `watch` one from auto-refresh, plus a one-shot `stream`
    /// one from a manual refresh — `bytes.lines` deadlocks the second
    /// reader indefinitely (confirmed by a standalone repro: `stream`
    /// starts, `watch` is already running, `stream` never yields a single
    /// line or completes). `readabilityHandler` doesn't have this problem.
    /// (This deadlock is exactly why manual refresh now prefers writing to
    /// an already-running `watch` process's stdin over spawning a second
    /// concurrent process at all — see `refresh()`.)
    private nonisolated static func streamLines(
        url: URL,
        args: [String]
    ) -> (stream: AsyncThrowingStream<String, Error>, stdin: FileHandle) {
        let inPipe = Pipe()
        let stream = AsyncThrowingStream<String, Error> { continuation in
            let process = Process()
            process.executableURL = url
            process.arguments = args
            let outPipe = Pipe()
            process.standardOutput = outPipe
            process.standardError = Pipe()
            process.standardInput = inPipe

            let buffer = LineBuffer()
            outPipe.fileHandleForReading.readabilityHandler = { handle in
                let data = handle.availableData
                guard !data.isEmpty else {
                    handle.readabilityHandler = nil
                    return
                }
                for line in buffer.appendAndExtractLines(data) {
                    continuation.yield(line)
                }
            }

            process.terminationHandler = { _ in
                outPipe.fileHandleForReading.readabilityHandler = nil
                continuation.finish()
            }

            do {
                try process.run()
            } catch {
                continuation.finish(throwing: error)
            }

            continuation.onTermination = { _ in
                outPipe.fileHandleForReading.readabilityHandler = nil
                if process.isRunning { process.terminate() }
            }
        }
        return (stream, inPipe.fileHandleForWriting)
    }

    /// JSON-encodes `command` as a single NDJSON line and writes it to
    /// `stdin`. Errors are swallowed (matches the fire-and-forget nature of
    /// `WatchCommand`: if this write is lost, the next manual refresh or
    /// the periodic poll recovers).
    private static func send(_ command: WatchCommand, to stdin: FileHandle) {
        guard var data = try? JSONEncoder().encode(command) else { return }
        data.append(UInt8(ascii: "\n"))
        try? stdin.write(contentsOf: data)
    }

    private static func locateBinary() -> URL? {
        let fm = FileManager.default

        if let envPath = ProcessInfo.processInfo.environment["NETCHECK_BIN"], fm.fileExists(atPath: envPath) {
            return URL(fileURLWithPath: envPath)
        }

        if let bundled = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("netcheck"),
            fm.fileExists(atPath: bundled.path)
        {
            return bundled
        }

        let candidates = [
            "target/release/netcheck",
            "target/debug/netcheck",
            "../../target/release/netcheck",
            "../../target/debug/netcheck",
        ]
        for candidate in candidates where fm.fileExists(atPath: candidate) {
            return URL(fileURLWithPath: candidate).absoluteURL
        }

        if let pathEnv = ProcessInfo.processInfo.environment["PATH"] {
            for dir in pathEnv.split(separator: ":") {
                let candidate = "\(dir)/netcheck"
                if fm.fileExists(atPath: candidate) {
                    return URL(fileURLWithPath: candidate)
                }
            }
        }

        return nil
    }
}
