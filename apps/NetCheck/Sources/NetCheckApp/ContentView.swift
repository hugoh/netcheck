import AppKit
import NetStatus
import SwiftUI

struct StatusIcon: View {
    let ok: Bool

    var body: some View {
        Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle.fill")
            .foregroundStyle(ok ? .green : .red)
    }
}

/// Like `StatusIcon`, but for conditions that are optional rather than
/// expected to always be met — no VPN connected isn't a failure, so it's
/// shown neutral rather than as a red X.
struct OptionalStatusIcon: View {
    let on: Bool

    var body: some View {
        Image(systemName: on ? "checkmark.circle.fill" : "minus.circle")
            .foregroundStyle(on ? .green : .secondary)
    }
}

struct PanelBox<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        GroupBox(title) {
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct CollectingPlaceholder: View {
    var body: some View {
        Text("Collecting...")
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
    }
}

enum Tab: Int, CaseIterable {
    case overview = 1
    case dns = 2
    case reachability = 3
    case wifi = 4

    var label: String {
        switch self {
        case .overview: "Overview"
        case .dns: "DNS"
        case .reachability: "Reachability"
        case .wifi: "Wi-Fi"
        }
    }
}

struct ContentView: View {
    @ObservedObject var fetcher: StatusFetcher
    @State private var activeTab: Tab = .overview

    /// CFBundleShortVersionString, set by mise's build:app (dev, "dev-<sha>")
    /// or bundle:swiftui (release, the real tag) — see Info.plist.
    private static var appVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "dev"
    }

    var body: some View {
        VStack(spacing: 0) {
            Group {
                if fetcher.status.hasAny {
                    tabbedContent(fetcher.status)
                } else if let error = fetcher.errorMessage {
                    VStack(spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .font(.largeTitle)
                            .foregroundStyle(.orange)
                        Text(error).multilineTextAlignment(.center).padding()
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ProgressView("Collecting network status...")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(maxHeight: .infinity)

            Divider()
            footer
        }
        .toolbar {
            ToolbarItem {
                Picker("", selection: $activeTab) {
                    ForEach(Tab.allCases, id: \.self) { tab in
                        Text("\(tab.label)  ⌘\(tab.rawValue)").tag(tab)
                    }
                }
                .pickerStyle(.segmented)
            }
            ToolbarItem {
                Button {
                    fetcher.refresh()
                } label: {
                    Label {
                        Text("Refresh  ") + Text("⌘R").foregroundColor(.gray)
                    } icon: {
                        if fetcher.isRefreshing {
                            ProgressView().controlSize(.small)
                        } else {
                            Image(systemName: "arrow.clockwise")
                        }
                    }
                }
                .keyboardShortcut("r", modifiers: [.command])
            }
        }
        .background(tabKeyShortcuts)
        .background(tabNavigationShortcuts)
    }

    private var footer: some View {
        HStack(spacing: 12) {
            if let confidence = fetcher.status.confidence {
                Label(confidence.label, systemImage: confidence.icon)
                    .foregroundStyle(confidence.color)
            }

            if fetcher.isRefreshing {
                HStack(spacing: 4) {
                    ProgressView().controlSize(.small)
                    Text("Refreshing")
                }
                .foregroundStyle(.cyan)
            }

            Text(Self.appVersion)
                .foregroundStyle(.secondary)

            if fetcher.status.captivePortal == .detected {
                Label("Captive portal", systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.yellow)
            }

            Spacer()

            Toggle(isOn: Binding(get: { fetcher.autoRefreshEnabled }, set: { _ in fetcher.toggleAutoRefresh() })) {
                Text("Auto-refresh  ") + Text("⌘A").foregroundColor(.gray)
            }
            .keyboardShortcut("a", modifiers: [.command])

            if let updated = fetcher.lastUpdated {
                TimelineView(.periodic(from: updated, by: 1)) { context in
                    Text(updatedAgoText(now: context.date, updated: updated))
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .font(.callout)
    }

    /// Hidden buttons carrying keyboard shortcuts Cmd+1-4, echoing
    /// netcheck-tui/-gui's number-key tab switching.
    private var tabKeyShortcuts: some View {
        ForEach(Tab.allCases, id: \.self) { tab in
            Button("") { activeTab = tab }
                .keyboardShortcut(KeyEquivalent(Character("\(tab.rawValue)")), modifiers: [.command])
                .opacity(0)
                .frame(width: 0, height: 0)
        }
    }

    private var tabNavigationShortcuts: some View {
        Group {
            Button("") { moveTab(by: -1) }
                .keyboardShortcut("[", modifiers: [.command, .shift])
            Button("") { moveTab(by: 1) }
                .keyboardShortcut("]", modifiers: [.command, .shift])
        }
        .opacity(0)
        .frame(width: 0, height: 0)
    }

    /// True while a manual refresh is in flight and `key`'s row hasn't
    /// gotten this refresh's fresh value yet — see
    /// `PartialNetworkStatus.isPending`. Gated on `isRefreshing` so a row
    /// that's simply never been touched by *any* refresh (e.g. right after
    /// launch) doesn't read as "pending forever": it shows the normal
    /// collecting/empty state instead until the first refresh actually
    /// starts.
    func isPending(_ key: String) -> Bool {
        guard fetcher.isRefreshing, let generation = fetcher.pendingRefreshGeneration else {
            return false
        }
        return fetcher.status.isPending(key, asOf: generation)
    }

    @ViewBuilder
    func statusOrSpinner(ok: Bool, pending: Bool) -> some View {
        if pending {
            ProgressView().controlSize(.small)
        } else {
            StatusIcon(ok: ok)
        }
    }

    private func moveTab(by offset: Int) {
        let all = Tab.allCases
        guard let index = all.firstIndex(of: activeTab) else { return }
        let newIndex = (index + offset + all.count) % all.count
        activeTab = all[newIndex]
    }

    @ViewBuilder
    private func tabbedContent(_ status: PartialNetworkStatus) -> some View {
        switch activeTab {
        case .overview: overviewTab(status)
        case .dns: dnsTab(status)
        case .reachability: reachabilityTab(status)
        case .wifi:
            PanelBox(title: "Wi-Fi") {
                wifiDetail(identity: status.wifiIdentity, radio: status.wifiRadio)
            }
            .padding(12)
        }
    }
}
