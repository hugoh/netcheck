import Foundation

/// Accumulates raw pipe reads and splits off complete newline-terminated
/// lines. `@unchecked Sendable`: a `FileHandle.readabilityHandler` is
/// invoked serially by the OS for a given handle — never concurrently with
/// itself — so a single instance is safe to mutate across those calls
/// despite not being actor-isolated.
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

/// Subprocess plumbing for `StatusFetcher` — running the `netcheck` binary,
/// streaming its stdout, writing commands to its stdin, and finding it on
/// disk. Split out so the type body stays focused on published state and
/// event handling. These are all stateless statics (no `self`), so the
/// split costs nothing beyond `internal` visibility.
extension StatusFetcher {
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
    nonisolated static func streamLines(
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
    static func send(_ command: WatchCommand, to stdin: FileHandle) {
        guard var data = try? JSONEncoder().encode(command) else { return }
        data.append(UInt8(ascii: "\n"))
        try? stdin.write(contentsOf: data)
    }

    static func locateBinary() -> URL? {
        let fm = FileManager.default

        if let envPath = ProcessInfo.processInfo.environment["NETCHECK_BIN"], fm.fileExists(atPath: envPath) {
            return URL(fileURLWithPath: envPath)
        }

        if let bundled = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("netcheck"),
            fm.fileExists(atPath: bundled.path) {
            return bundled
        }

        let candidates = [
            "target/release/netcheck",
            "target/debug/netcheck",
            "../../target/release/netcheck",
            "../../target/debug/netcheck"
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
