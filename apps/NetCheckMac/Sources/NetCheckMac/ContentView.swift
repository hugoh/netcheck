import SwiftUI

struct StatusIcon: View {
    let ok: Bool

    var body: some View {
        Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle.fill")
            .foregroundStyle(ok ? .green : .red)
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

struct ContentView: View {
    @ObservedObject var fetcher: StatusFetcher

    var body: some View {
        Group {
            if let status = fetcher.status {
                statusGrid(status)
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
                Text("netcheck").font(.headline)
            }
            ToolbarItem {
                Button {
                    fetcher.refresh()
                } label: {
                    Label("Refresh  [r]", systemImage: "arrow.clockwise")
                }
                .keyboardShortcut("r", modifiers: [])
            }
            ToolbarItem {
                Toggle(
                    "Auto-refresh  [a]",
                    isOn: Binding(get: { fetcher.autoRefreshEnabled }, set: { _ in fetcher.toggleAutoRefresh() })
                )
                .keyboardShortcut("a", modifiers: [])
            }
            ToolbarItem {
                if let updated = fetcher.lastUpdated {
                    Text("Updated \(updated, style: .relative) ago")
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder
    private func statusGrid(_ status: NetworkStatus) -> some View {
        HStack(alignment: .top, spacing: 12) {
            PanelBox(title: "Interfaces") {
                List(
                    status.interfaces
                        .filter { !$0.loopback }
                        .sorted { classify($0) < classify($1) }
                ) { iface in
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
            }

            VStack(spacing: 12) {
                PanelBox(title: "VPN / Tunnel") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Primary interface: \(status.vpn.primaryInterface ?? "unknown")")
                        HStack {
                            StatusIcon(ok: status.vpn.connected)
                            Text("VPN connected: \(status.vpn.connected ? "yes" : "no")")
                        }
                        Text("Split tunnel: \(status.vpn.splitTunnel ? "yes" : "no")")
                        Text("Split DNS: \(status.splitDns ? "yes" : "no")")
                        if !status.vpn.tunnels.isEmpty {
                            Text("Tunnels: \(status.vpn.tunnels.joined(separator: ", "))")
                        }
                    }
                    .padding(8)
                }
                .frame(height: 150)

                PanelBox(title: "DNS resolvers") {
                    List(status.resolvers.filter { !$0.nameservers.isEmpty }) { r in
                        HStack {
                            StatusIcon(ok: r.reachable)
                            Text(r.domain ?? r.searchDomains.first ?? "*")
                            Spacer()
                            Text(r.ifName ?? "any").foregroundStyle(.secondary)
                            Text(r.nameservers.joined(separator: ", ")).foregroundStyle(.secondary)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                }

                PanelBox(title: "DNS resolution") {
                    List(status.resolution) { r in
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
                }
            }

            VStack(spacing: 12) {
                PanelBox(title: "Reachability (IPs)") {
                    List(status.reachability) { p in
                        HStack {
                            StatusIcon(ok: p.reachable)
                            Text(p.target)
                            Spacer()
                            Text(p.rttMs.map { String(format: "%.1f ms", $0) } ?? "timeout")
                                .foregroundStyle(.secondary)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                }

                PanelBox(title: "Reachability (domains, TCP:443)") {
                    List(status.domainReachability) { c in
                        HStack {
                            StatusIcon(ok: c.reachable)
                            Text(c.target)
                            Spacer()
                            Text(c.rttMs.map { String(format: "%.1f ms", $0) } ?? "unreachable")
                                .foregroundStyle(.secondary)
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                }
            }
        }
        .padding(12)
    }
}
