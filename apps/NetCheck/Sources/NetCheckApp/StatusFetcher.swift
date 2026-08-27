import Foundation

/// Runs the `netcheck` CLI's `stream`/`watch` subcommands and decodes their
/// NDJSON output line by line as it arrives, publishing each field the
/// moment it's ready instead of waiting for a full snapshot — mirrors
/// netcheck-tui/-gui's streamed `PartialStatus` model. Reuses the
/// already-tested Rust `netstatus` core instead of re-implementing any
/// diagnostics (or refresh cadence) in Swift: auto-refresh runs `watch`,
/// which re-checks on OS network-config changes plus an adaptive-interval
/// poll as a safety net, instead of a fixed Swift-side timer.
@MainActor
final class StatusFetcher: ObservableObject {
    @Published var status = PartialNetworkStatus()
    @Published var lastUpdated: Date?
    @Published var autoRefreshEnabled = true
    @Published var errorMessage: String?

    private var refreshTask: Task<Void, Never>?
    private var refreshGeneration = 0
    private var watchTask: Task<Void, Never>?
    private let binaryURL: URL?

    init() {
        binaryURL = Self.locateBinary()
        // No separate manual `refresh()` here: `netcheck watch`'s own first
        // fire is already a full snapshot (mirrors the TUI, which dropped
        // its redundant initial-fetch thread for the same reason), so
        // starting it directly avoids running two independent full
        // collections at startup.
        startWatching()
    }

    func toggleAutoRefresh() {
        autoRefreshEnabled.toggle()
        if autoRefreshEnabled {
            startWatching()
        } else {
            watchTask?.cancel()
            watchTask = nil
        }
    }

    /// Starts the persistent `netcheck watch` subprocess and merges each
    /// NDJSON line it emits, for as long as auto-refresh stays on. Unlike
    /// `refresh()`, this runs one long-lived process rather than restarting
    /// on a timer — the Rust side owns the config-change/poll cadence.
    private func startWatching() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }

        watchTask = Task {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            do {
                for try await line in Self.streamLines(url: binaryURL, args: ["watch"]) {
                    guard !line.isEmpty, let data = line.data(using: .utf8) else { continue }
                    guard let envelope = try? decoder.decode(StatusFieldEnvelope.self, from: data) else {
                        continue
                    }
                    status.merge(envelope)
                    lastUpdated = Date()
                    errorMessage = nil
                }
            } catch is CancellationError {
                // Auto-refresh was toggled off; nothing to report.
            } catch {
                errorMessage = "Failed to run netcheck watch: \(error.localizedDescription)"
            }
        }
    }

    func refresh() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }

        refreshTask?.cancel()
        refreshGeneration += 1
        let generation = refreshGeneration
        refreshTask = Task {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            do {
                for try await line in Self.streamLines(url: binaryURL, args: ["stream"]) {
                    guard generation == refreshGeneration else { return }
                    guard !line.isEmpty, let data = line.data(using: .utf8) else { continue }
                    guard let envelope = try? decoder.decode(StatusFieldEnvelope.self, from: data) else {
                        continue
                    }
                    status.merge(envelope)
                    lastUpdated = Date()
                    errorMessage = nil
                }
            } catch is CancellationError {
                // Superseded by a newer refresh; nothing to report.
            } catch {
                guard generation == refreshGeneration else { return }
                errorMessage = "Failed to run netcheck: \(error.localizedDescription)"
            }
        }
    }

    /// Runs `netcheck stream` and yields each line of its stdout as it's
    /// written — no waiting for the process to exit, no blocking reads.
    private nonisolated static func streamLines(url: URL, args: [String]) -> AsyncThrowingStream<String, Error> {
        AsyncThrowingStream { continuation in
            let process = Process()
            process.executableURL = url
            process.arguments = args
            let outPipe = Pipe()
            process.standardOutput = outPipe
            process.standardError = Pipe()

            let readTask = Task {
                do {
                    try process.run()
                    for try await line in outPipe.fileHandleForReading.bytes.lines {
                        continuation.yield(line)
                    }
                    process.waitUntilExit()
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }

            continuation.onTermination = { _ in
                readTask.cancel()
                if process.isRunning { process.terminate() }
            }
        }
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
