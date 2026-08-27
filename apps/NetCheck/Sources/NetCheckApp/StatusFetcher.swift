import AppKit
import Foundation

/// Runs the `netcheck` CLI's `stream`/`watch` subcommands and decodes their
/// NDJSON `StreamEvent` output line by line as it arrives, publishing each
/// field the moment it's ready instead of waiting for a full snapshot —
/// mirrors netcheck-tui/-gui's streamed `PartialStatus` model. Reuses the
/// already-tested Rust `netstatus` core instead of re-implementing any
/// diagnostics (or refresh cadence) in Swift: auto-refresh runs `watch`,
/// which re-checks on OS network-config changes plus an adaptive-interval
/// poll as a safety net, instead of a fixed Swift-side timer.
///
/// The subprocess/pipe plumbing (`streamLines`, `send`, `locateBinary`,
/// `LineBuffer`) lives in `StatusFetcherProcess.swift`.
@MainActor
final class StatusFetcher: ObservableObject {
    /// Wire protocol version this app understands — must match
    /// `StreamEvent::Hello.protocol_version` from the `netcheck` binary.
    static let supportedProtocolVersion = 1

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
    /// The wire generation of the manual refresh currently in flight, once
    /// its `CheckStarted` has echoed our token back — `nil` before that (a
    /// few ms) and whenever no manual refresh is pending. Drives which rows
    /// the UI marks pending (`PartialNetworkStatus.isPending`).
    @Published private(set) var pendingRefreshGeneration: Int?

    /// Only used as the auto-refresh-off fallback — a one-shot `netcheck
    /// stream` process for a manual refresh, spawned when there's no
    /// persistent `watch` process to signal instead. `nil` whenever
    /// auto-refresh is on.
    private var refreshTask: Task<Void, Never>?
    /// Local refresh counter for the fallback path only. The `watch` path
    /// uses the authoritative wire generation instead (see `merge`).
    private var fallbackGeneration = 0
    /// The correlation token of the manual refresh in flight, matched
    /// against `CheckStarted.token` to learn its wire generation.
    private var pendingRefreshToken: String?
    private var watchTask: Task<Void, Never>?
    private var watchdogTask: Task<Void, Never>?
    private var restartTask: Task<Void, Never>?
    /// Last time any `StreamEvent` (including `Heartbeat`) arrived — the
    /// watchdog restarts `watch` if this goes stale.
    private var lastEventAt = Date()
    /// Backoff before the next `watch` restart; grows on repeated failures,
    /// resets on a healthy `Hello`.
    private var restartBackoff: Duration = .seconds(1)
    /// Cleared to `false` on a `Hello` whose `protocol_version` this app
    /// doesn't understand — field events are then ignored rather than
    /// decoded against a format that may have changed shape.
    private var protocolCompatible = true
    /// stdin of the currently-running `watch` process, if any — `refresh()`
    /// writes a `WatchCommand` line here instead of spawning a second
    /// concurrent subprocess when this is non-nil.
    private var watchStdin: FileHandle?
    private let binaryURL: URL?
    private var terminationObserver: NSObjectProtocol?

    convenience init() {
        self.init(binary: Self.locateBinary())
    }

    /// Designated initializer — `binary` is the `netcheck` executable to
    /// run. Production uses `locateBinary()`; tests pass a deterministic
    /// fixture that emits canned `StreamEvent` NDJSON.
    init(binary: URL?) {
        binaryURL = binary
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
                self?.watchdogTask?.cancel()
                self?.restartTask?.cancel()
            }
        }
    }

    func toggleAutoRefresh() {
        autoRefreshEnabled.toggle()
        if autoRefreshEnabled {
            restartBackoff = .seconds(1)
            startWatching()
        } else {
            watchTask?.cancel()
            watchTask = nil
            watchdogTask?.cancel()
            watchdogTask = nil
            restartTask?.cancel()
            restartTask = nil
            watchStdin = nil
        }
    }

    /// Starts the persistent `netcheck watch` subprocess and merges each
    /// `StreamEvent` line it emits, for as long as auto-refresh stays on.
    /// Unlike the auto-refresh-off fallback in `refresh()`, this runs one
    /// long-lived process rather than restarting on a timer — the Rust
    /// side owns the config-change/poll cadence, and `refresh()` triggers
    /// an immediate check by writing to this same process's stdin instead
    /// of spawning a second one. If the process dies or stops responding,
    /// `scheduleRestart` brings it back with backoff.
    private func startWatching() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }

        let (stream, stdin) = Self.streamLines(url: binaryURL, args: ["watch"])
        watchStdin = stdin
        lastEventAt = Date()
        protocolCompatible = true
        startWatchdog()
        watchTask = Task {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            do {
                for try await line in stream {
                    guard !line.isEmpty, let data = line.data(using: .utf8) else { continue }
                    guard let event = try? decoder.decode(StreamEventEnvelope.self, from: data) else {
                        continue
                    }
                    handle(event)
                }
                if autoRefreshEnabled {
                    scheduleRestart(reason: "netcheck watch exited unexpectedly")
                }
            } catch is CancellationError {
                // Auto-refresh was toggled off; nothing to report.
            } catch {
                if autoRefreshEnabled {
                    scheduleRestart(reason: "netcheck watch failed: \(error.localizedDescription)")
                }
            }
            watchStdin = nil
        }
    }

    private func handleHello(_ hello: StreamEventEnvelope.Hello) {
        restartBackoff = .seconds(1)
        if hello.protocolVersion != Self.supportedProtocolVersion {
            protocolCompatible = false
            errorMessage = "netcheck speaks wire protocol \(hello.protocolVersion); " +
                "this app expects \(Self.supportedProtocolVersion). Update netcheck."
        } else if hello.configWatcher == "unavailable" {
            errorMessage = "Network-change watching is unavailable; " +
                "falling back to periodic polling."
        }
    }

    private func handle(_ event: StreamEventEnvelope) {
        lastEventAt = Date()

        if let hello = event.hello {
            handleHello(hello)
            return
        }

        guard protocolCompatible else { return }

        if let started = event.checkStarted {
            if let token = started.token, token == pendingRefreshToken {
                pendingRefreshGeneration = started.generation
            }
            return
        }

        if let f = event.field {
            status.merge(f.field, generation: f.generation)
            lastUpdated = Date()
            errorMessage = nil
            return
        }

        if let complete = event.checkComplete {
            if let pending = pendingRefreshGeneration, complete.generation >= pending {
                isRefreshing = false
                pendingRefreshToken = nil
                pendingRefreshGeneration = nil
            }
            return
        }

        if event.heartbeat != nil {
            return
        }

        if let wireError = event.error {
            errorMessage = wireError.message
            if wireError.fatal, autoRefreshEnabled {
                scheduleRestart(reason: wireError.message)
            }
        }
    }

    /// Restarts the `watch` subprocess after a backoff — used when it exits,
    /// errors fatally, or stops emitting events (watchdog). The backoff
    /// grows on each consecutive restart (capped at 5s) and resets on the
    /// next healthy `Hello`.
    private func scheduleRestart(reason: String) {
        errorMessage = reason
        watchTask?.cancel()
        watchTask = nil
        watchStdin = nil
        watchdogTask?.cancel()
        watchdogTask = nil
        restartTask?.cancel()

        let backoff = restartBackoff
        restartBackoff = min(restartBackoff * 2, .seconds(5))
        restartTask = Task { [weak self] in
            try? await Task.sleep(for: backoff)
            guard !Task.isCancelled, let self, self.autoRefreshEnabled else { return }
            self.startWatching()
        }
    }

    private func startWatchdog() {
        watchdogTask?.cancel()
        watchdogTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(10))
                guard let self, self.autoRefreshEnabled else { continue }
                if Date().timeIntervalSince(self.lastEventAt) > 45 {
                    self.scheduleRestart(reason: "netcheck watch stopped responding")
                    return
                }
            }
        }
    }

    func refresh() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }

        isRefreshing = true

        if let watchStdin {
            // A `watch` process is already running — trigger it via stdin
            // with a correlation token instead of spawning a second
            // concurrent subprocess (real deadlock hazard, see streamLines'
            // doc comment). Completion is detected in `handle` when the
            // `CheckComplete` for this token's generation arrives.
            let token = UUID().uuidString
            pendingRefreshToken = token
            pendingRefreshGeneration = nil
            Self.send(.refreshToken(token), to: watchStdin)
            return
        }

        // Auto-refresh is off — no persistent process to signal, so fall
        // back to a one-shot `netcheck stream`. Safe by construction:
        // nothing else reads a pipe concurrently while `watchTask` /
        // `watchStdin` are nil.
        refreshTask?.cancel()
        fallbackGeneration += 1
        let generation = fallbackGeneration
        pendingRefreshGeneration = generation
        let (stream, _) = Self.streamLines(url: binaryURL, args: ["stream"])
        refreshTask = Task {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase

            do {
                for try await line in stream {
                    guard generation == fallbackGeneration else { return }
                    guard !line.isEmpty, let data = line.data(using: .utf8) else { continue }
                    guard let event = try? decoder.decode(StreamEventEnvelope.self, from: data),
                          let f = event.field
                    else { continue }
                    status.merge(f.field, generation: generation)
                    lastUpdated = Date()
                    errorMessage = nil
                }
            } catch is CancellationError {
                // Superseded by a newer refresh; nothing to report.
            } catch {
                guard generation == fallbackGeneration else { return }
                errorMessage = "Failed to run netcheck: \(error.localizedDescription)"
            }
            if generation == fallbackGeneration {
                isRefreshing = false
                pendingRefreshToken = nil
                pendingRefreshGeneration = nil
            }
        }
    }
}
