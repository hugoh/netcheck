import Foundation
import NetStatus

/// Drives `NetStatus.collectStreaming` and folds each `StatusField` into a
/// `PartialNetworkStatus` the moment it arrives. Auto-refresh re-runs on
/// every OS network-path change (`NetworkChangeMonitor`) plus an
/// adaptive-interval poll as a safety net.
@MainActor
final class StatusFetcher: ObservableObject {
    @Published var status = PartialNetworkStatus()
    @Published var lastUpdated: Date?
    @Published var autoRefreshEnabled = true
    @Published var errorMessage: String?
    /// True for the duration of a manual (⌘R) refresh — visible proof the
    /// refresh actually ran.
    @Published var isRefreshing = false
    /// The generation of the manual refresh in flight — drives which rows
    /// the UI marks pending (`PartialNetworkStatus.isPending`).
    @Published private(set) var pendingRefreshGeneration: Int?

    /// Poll cadence: a longer wait while the connection looks healthy, a
    /// shorter one once it's degraded so recovery is noticed quickly.
    private let healthyInterval: Duration = .seconds(60)
    private let degradedInterval: Duration = .seconds(10)
    /// Every Nth poll tick runs a full check instead of confidence-only, to
    /// catch a descriptive-config change that produced no path event (a
    /// captive-portal sign-in, a silent resolver swap).
    private static let pollFullEvery = 5
    /// Collapses a burst of path updates (a VPN connecting fires several)
    /// into one check.
    private let changeDebounce: Duration = .milliseconds(300)

    private var generation = 0
    private var currentCheck: Task<Void, Never>?
    private var autoLoop: Task<Void, Never>?
    private var connectionDegraded = false

    init() {
        // The first check is a full one; the auto-loop's own probes don't
        // duplicate it.
        runCheck(scope: .full, manual: false)
        startAutoLoop()
    }

    func toggleAutoRefresh() {
        autoRefreshEnabled.toggle()
        if autoRefreshEnabled {
            startAutoLoop()
        } else {
            autoLoop?.cancel()
            autoLoop = nil
        }
    }

    func refresh() {
        isRefreshing = true
        runCheck(scope: .full, manual: true)
    }

    private func startAutoLoop() {
        autoLoop?.cancel()
        autoLoop = Task { [weak self] in
            await withTaskGroup(of: Void.self) { group in
                group.addTask { await self?.watchPathChanges() }
                group.addTask { await self?.pollLoop() }
            }
        }
    }

    private func watchPathChanges() async {
        let monitor = NetworkChangeMonitor()
        for await _ in monitor.changes() {
            try? await Task.sleep(for: changeDebounce)
            guard !Task.isCancelled else { return }
            runCheck(scope: .full, manual: false)
        }
    }

    private func pollLoop() async {
        var tick = 0
        while !Task.isCancelled {
            try? await Task.sleep(for: connectionDegraded ? degradedInterval : healthyInterval)
            guard !Task.isCancelled else { return }
            tick += 1
            let scope: CollectScope = tick % Self.pollFullEvery == 0 ? .full : .confidence
            runCheck(scope: scope, manual: false)
        }
    }

    private func runCheck(scope: CollectScope, manual: Bool) {
        currentCheck?.cancel()
        if pendingRefreshGeneration != nil, !manual {
            isRefreshing = false
            pendingRefreshGeneration = nil
        }
        generation += 1
        let generation = generation
        if manual {
            pendingRefreshGeneration = generation
        }

        currentCheck = Task { [weak self] in
            for await field in NetStatus.collectStreaming(scope: scope) {
                guard !Task.isCancelled else { return }
                self?.apply(field, generation: generation, manual: manual)
            }
            self?.completeCheck(generation: generation, manual: manual)
        }
    }

    private func apply(_ field: StatusField, generation: Int, manual: Bool) {
        guard generation == self.generation else { return }
        status.merge(field, generation: generation)
        lastUpdated = Date()
        errorMessage = nil

        if let confidence = status.confidence {
            connectionDegraded = confidence != .online
        }
        if manual, status.isRefreshComplete(asOf: generation) {
            isRefreshing = false
            pendingRefreshGeneration = nil
        }
    }

    private func completeCheck(generation: Int, manual: Bool) {
        guard generation == self.generation, manual else { return }
        isRefreshing = false
        pendingRefreshGeneration = nil
    }
}
