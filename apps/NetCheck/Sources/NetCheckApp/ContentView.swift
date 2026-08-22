import AppKit
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
        case .overview: return "Overview"
        case .dns: return "DNS"
        case .reachability: return "Reachability"
        case .wifi: return "Wi-Fi"
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
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Text(Self.appVersion).foregroundStyle(.secondary)
            }
            ToolbarItem {
                Picker("", selection: $activeTab) {
                    ForEach(Tab.allCases, id: \.self) { tab in
                        Text("[⌘\(tab.rawValue)] \(tab.label)").tag(tab)
                    }
                }
                .pickerStyle(.segmented)
            }
            ToolbarItem {
                Button {
                    fetcher.refresh()
                } label: {
                    Label("Refresh  [⌘R]", systemImage: "arrow.clockwise")
                }
                .keyboardShortcut("r", modifiers: [.command])
            }
            ToolbarItem {
                Toggle(
                    "Auto-refresh  [⌘A]",
                    isOn: Binding(get: { fetcher.autoRefreshEnabled }, set: { _ in fetcher.toggleAutoRefresh() })
                )
                .keyboardShortcut("a", modifiers: [.command])
            }
            ToolbarItem {
                if let updated = fetcher.lastUpdated {
                    Text("Updated \(updated, style: .relative) ago")
                        .foregroundStyle(.secondary)
                }
            }
        }
        .background(tabKeyShortcuts)
        .background(quitKeyShortcut)
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

    /// Plain `q` quits, matching netcheck-tui/-gui — separate from Cmd+Q,
    /// which SwiftUI already wires to the standard app menu.
    private var quitKeyShortcut: some View {
        Button("") { NSApp.terminate(nil) }
            .keyboardShortcut("q", modifiers: [])
            .opacity(0)
            .frame(width: 0, height: 0)
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

    @ViewBuilder
    private func overviewTab(_ status: PartialNetworkStatus) -> some View {
        HStack(alignment: .top, spacing: 12) {
            PanelBox(title: "Interfaces") {
                if let interfaces = status.interfaces {
                    List(interfaces.filter { !$0.loopback }.sorted { classify($0) < classify($1) }) { iface in
                        let cls = classify(iface)
                        HStack {
                            Text(iface.name).font(.system(.body, design: .monospaced))
                            Text(cls.label)
                                .font(.caption)
                                .foregroundStyle(cls.color)
                            Spacer()
                            Text(iface.addresses.isEmpty ? "—" : iface.addresses.joined(separator: ", "))
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                                .truncationMode(.tail)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                } else {
                    CollectingPlaceholder()
                }
            }

            VStack(spacing: 12) {
                PanelBox(title: "VPN / Tunnel") {
                    if let vpn = status.vpn {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Primary interface: \(vpn.primaryInterface ?? "unknown")")
                            HStack {
                                OptionalStatusIcon(on: vpn.connected)
                                Text("VPN connected: \(vpn.connected ? "yes" : "no")")
                            }
                            Text("Split tunnel: \(vpn.splitTunnel ? "yes" : "no")")
                            Text("Split DNS: \(status.splitDns.map { $0 ? "yes" : "no" } ?? "collecting...")")
                            if !vpn.tunnels.isEmpty {
                                Text("Tunnels: \(vpn.tunnels.joined(separator: ", "))")
                            }
                            if let subnets = vpn.routedSubnets, !subnets.isEmpty {
                                Text("Routed subnets: \(subnets.joined(separator: ", "))")
                            }
                            if let resolvers = status.resolvers {
                                let domains = vpnScopedDomains(resolvers)
                                if !domains.isEmpty {
                                    Text("VPN domains: \(domains.joined(separator: ", "))")
                                }
                            }
                        }
                        .padding(8)
                    } else {
                        CollectingPlaceholder()
                    }
                }
                .frame(height: 170)

                PanelBox(title: "Proxy") {
                    if let proxy = status.proxy {
                        VStack(alignment: .leading, spacing: 6) {
                            proxyEndpointLine("HTTP", proxy.http)
                            proxyEndpointLine("HTTPS", proxy.https)
                            proxyEndpointLine("SOCKS", proxy.socks)
                            Text(proxy.pacUrl.map { "PAC: \($0)" } ?? "PAC: off")
                            if !proxy.exceptions.isEmpty {
                                Text("Exceptions: \(proxy.exceptions.joined(separator: ", "))")
                            }
                        }
                        .padding(8)
                    } else {
                        CollectingPlaceholder()
                    }
                }
                .frame(height: 130)

                PanelBox(title: "IP stack") {
                    Text(ipStackLabel(status.ipStack))
                        .padding(8)
                }
                .frame(height: 60)
            }
        }
        .padding(12)
    }

    private func proxyEndpointLine(_ label: String, _ endpoint: ProxyEndpoint) -> some View {
        Text(
            endpoint.enabled
                ? "\(label): \(endpoint.host ?? "?"):\(endpoint.port.map(String.init) ?? "?")"
                : "\(label): off"
        )
    }

    private func ipStackLabel(_ ipStack: IpStack?) -> String {
        guard let ipStack else { return "Collecting..." }
        switch ipStack {
        case .ipv4Only: return "IPv4 only"
        case .ipv6Only: return "IPv6 only"
        case .dualStack: return "Dual-stack (IPv4 + IPv6)"
        case .none: return "No routable address"
        }
    }

    @ViewBuilder
    private func dnsTab(_ status: PartialNetworkStatus) -> some View {
        HStack(alignment: .top, spacing: 12) {
            PanelBox(title: "DNS resolvers") {
                if let resolvers = status.resolvers {
                    List(resolvers.filter { !$0.nameservers.isEmpty }) { r in
                        HStack {
                            StatusIcon(ok: r.reachable)
                            Text(r.domain ?? r.searchDomains.first ?? "*")
                            Spacer()
                            Text(r.ifName ?? "any").foregroundStyle(.secondary)
                            Text(r.nameservers.joined(separator: ", ")).foregroundStyle(.secondary)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                } else {
                    CollectingPlaceholder()
                }
            }

            PanelBox(title: "DNS resolution") {
                if let resolution = status.resolution {
                    List(resolution) { r in
                        HStack {
                            StatusIcon(ok: r.resolved)
                            Text(r.domain)
                            Spacer()
                            Text(r.durationMs.map { String(format: "%.1f ms", $0) } ?? "")
                                .foregroundStyle(.secondary)
                            Text(r.addresses.first ?? "").foregroundStyle(.secondary)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                } else {
                    CollectingPlaceholder()
                }
            }
        }
        .padding(12)
    }

    @ViewBuilder
    private func reachabilityTab(_ status: PartialNetworkStatus) -> some View {
        HStack(alignment: .top, spacing: 12) {
            PanelBox(title: "Reachability (IPv4)") {
                pingList(status.reachability)
            }
            PanelBox(title: "Reachability (IPv6)") {
                pingList(status.reachabilityV6)
            }
            PanelBox(title: "Reachability (domains, TCP:443)") {
                if let results = status.domainReachability {
                    List(results) { c in
                        HStack {
                            StatusIcon(ok: c.reachable)
                            Text(c.target)
                            Spacer()
                            Text(c.rttMs.map { String(format: "%.1f ms", $0) } ?? "unreachable")
                                .foregroundStyle(.secondary)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                } else {
                    CollectingPlaceholder()
                }
            }
        }
        .padding(12)
    }

    @ViewBuilder
    private func pingList(_ results: [PingResult]?) -> some View {
        if let results {
            List(results) { p in
                HStack {
                    StatusIcon(ok: p.reachable)
                    Text(p.target)
                    Spacer()
                    Text(p.rttMs.map { String(format: "%.1f ms", $0) } ?? "timeout")
                        .foregroundStyle(.secondary)
                }
            }
            .listStyle(.inset(alternatesRowBackgrounds: true))
        } else {
            CollectingPlaceholder()
        }
    }

    /// Wi-Fi status arrives as two independent, independently-paced probes:
    /// `radio` (channel/signal/noise/security/PHY-mode) is fast CoreWLAN, no
    /// shell-out, and typically shows up immediately; `identity` (SSID,
    /// connected-state) is slow `system_profiler`, commonly ~1s. Rendered
    /// separately so radio fields aren't held hostage by the slow SSID lookup.
    @ViewBuilder
    private func wifiDetail(identity: WifiIdentity?, radio: WifiRadio?) -> some View {
        if identity == nil && radio == nil {
            CollectingPlaceholder()
        } else if let identity, !identity.connected {
            Text("Not connected").foregroundStyle(.secondary).padding(8)
        } else {
            VStack(alignment: .leading, spacing: 6) {
                if let identity {
                    Text("SSID: \(identity.ssid ?? "-")")
                } else {
                    Text("SSID: collecting...")
                }
                if let radio {
                    Text("Channel: \(radio.channel ?? "-")")
                    Text("Signal: \(radio.signalDbm.map { "\($0) dBm" } ?? "-")")
                    Text("Noise: \(radio.noiseDbm.map { "\($0) dBm" } ?? "-")")
                    Text("Security: \(radio.security ?? "-")")
                    Text("PHY mode: \(radio.phyMode ?? "-")")
                } else {
                    Text("Channel/signal: collecting...")
                }
            }
            .padding(8)
        }
    }
}
