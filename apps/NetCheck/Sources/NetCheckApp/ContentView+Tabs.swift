import NetStatus
import SwiftUI

extension ContentView {
    func overviewTab(_ status: PartialNetworkStatus) -> some View {
        HStack(alignment: .top, spacing: 12) {
            interfacesPanel(status)

            VStack(spacing: 12) {
                PathPanel(path: status.path)
                    .frame(height: 100)
                vpnPanel(status)
                    .frame(height: 170)
                proxyPanel(status)
                    .frame(height: 130)
                ipStackPanel(status)
                    .frame(height: 60)
            }
        }
        .padding(12)
    }

    private func interfacesPanel(_ status: PartialNetworkStatus) -> some View {
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
    }

    private func vpnPanel(_ status: PartialNetworkStatus) -> some View {
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
                    if !vpn.routedSubnets.isEmpty {
                        Text("Routed subnets: \(vpn.routedSubnets.joined(separator: ", "))")
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
    }

    private func proxyPanel(_ status: PartialNetworkStatus) -> some View {
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
    }

    private func proxyEndpointLine(_ label: String, _ endpoint: ProxyEndpoint) -> some View {
        Text(
            endpoint.enabled
                ? "\(label): \(endpoint.host ?? "?"):\(endpoint.port.map(String.init) ?? "?")"
                : "\(label): off"
        )
    }

    private func ipStackPanel(_ status: PartialNetworkStatus) -> some View {
        PanelBox(title: "IP stack") {
            Text(ipStackLabel(status.ipStack))
                .padding(8)
        }
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

    func dnsTab(_ status: PartialNetworkStatus) -> some View {
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
                if status.resolution.isEmpty {
                    CollectingPlaceholder()
                } else {
                    List(status.resolution) { r in
                        HStack {
                            statusOrSpinner(ok: r.resolved, pending: isPending("resolution:\(r.domain)"))
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
        }
        .padding(12)
    }

    func reachabilityTab(_ status: PartialNetworkStatus) -> some View {
        HStack(alignment: .top, spacing: 12) {
            PanelBox(title: "Reachability (IPv4)") {
                pingList(status.reachability, group: "reachability")
            }
            PanelBox(title: "Reachability (IPv6)") {
                pingList(status.reachabilityV6, group: "reachabilityV6")
            }
            PanelBox(title: "Reachability (domains, TCP:443)") {
                if status.domainReachability.isEmpty {
                    CollectingPlaceholder()
                } else {
                    List(status.domainReachability) { c in
                        HStack {
                            statusOrSpinner(ok: c.reachable, pending: isPending("domainReachability:\(c.target)"))
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

    @ViewBuilder
    private func pingList(_ results: [PingResult], group: String) -> some View {
        if results.isEmpty {
            CollectingPlaceholder()
        } else {
            List(results) { p in
                HStack {
                    statusOrSpinner(ok: p.reachable, pending: isPending("\(group):\(p.target)"))
                    Text(p.target)
                    Spacer()
                    Text(p.rttMs.map { String(format: "%.1f ms", $0) } ?? "timeout")
                        .foregroundStyle(.secondary)
                }
            }
            .listStyle(.inset(alternatesRowBackgrounds: true))
        }
    }

    /// Wi-Fi status arrives as two independent, independently-paced probes:
    /// `radio` (channel/signal/noise/security/PHY-mode) is fast CoreWLAN, no
    /// shell-out, and typically shows up immediately; `identity` (SSID,
    /// connected-state) is slow `system_profiler`, commonly ~1s. Rendered
    /// separately so radio fields aren't held hostage by the slow SSID lookup.
    @ViewBuilder
    func wifiDetail(identity: WifiIdentity?, radio: WifiRadio?) -> some View {
        if identity == nil, radio == nil {
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
