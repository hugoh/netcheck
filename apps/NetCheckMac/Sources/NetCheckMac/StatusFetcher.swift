import Foundation

/// Runs the `netcheck` CLI's `status` subcommand and decodes its JSON output.
/// Reuses the already-tested Rust `netstatus` core instead of re-implementing
/// any diagnostics in Swift.
@MainActor
final class StatusFetcher: ObservableObject {
    @Published var status: NetworkStatus?
    @Published var lastUpdated: Date?
    @Published var autoRefreshEnabled = false
    @Published var errorMessage: String?

    private var timer: Timer?
    private let binaryURL: URL?

    init() {
        binaryURL = Self.locateBinary()
        refresh()
    }

    func toggleAutoRefresh() {
        autoRefreshEnabled.toggle()
        configureTimer()
    }

    private func configureTimer() {
        timer?.invalidate()
        timer = nil
        guard autoRefreshEnabled else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
    }

    func refresh() {
        guard let binaryURL else {
            errorMessage = "netcheck binary not found. Set NETCHECK_BIN, or build it with:\n" +
                "cargo build --release -p netcheck-cli"
            return
        }
        Task.detached(priority: .userInitiated) {
            do {
                let data = try Self.runProcess(url: binaryURL, args: ["status"])
                let decoder = JSONDecoder()
                decoder.keyDecodingStrategy = .convertFromSnakeCase
                let decoded = try decoder.decode(NetworkStatus.self, from: data)
                await MainActor.run { [weak self] in
                    self?.status = decoded
                    self?.lastUpdated = Date()
                    self?.errorMessage = nil
                }
            } catch {
                await MainActor.run { [weak self] in
                    self?.errorMessage = "Failed to run netcheck: \(error.localizedDescription)"
                }
            }
        }
    }

    private nonisolated static func runProcess(url: URL, args: [String]) throws -> Data {
        let process = Process()
        process.executableURL = url
        process.arguments = args
        let outPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = Pipe()
        try process.run()
        let data = outPipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return data
    }

    private static func locateBinary() -> URL? {
        let fm = FileManager.default

        if let envPath = ProcessInfo.processInfo.environment["NETCHECK_BIN"], fm.fileExists(atPath: envPath) {
            return URL(fileURLWithPath: envPath)
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
