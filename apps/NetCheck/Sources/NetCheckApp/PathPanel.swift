import NetStatus
import SwiftUI

/// The OS's own view of the primary route, from a one-shot `NWPath`
/// snapshot — reachability verdict, primary interface, and the
/// metered/Low-Data-Mode flags the address probes can't see.
struct PathPanel: View {
    let path: PathStatus?

    var body: some View {
        PanelBox(title: "Path (OS view)") {
            if let path {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Status: \(statusLabel(path.status))")
                    Text("Primary interface: \(interfaceLabel(path.primaryInterface))")
                    if let flags = flagsLabel(path) {
                        Text(flags).foregroundStyle(.secondary)
                    }
                }
                .padding(8)
            } else {
                CollectingPlaceholder()
            }
        }
    }

    private func statusLabel(_ status: PathState) -> String {
        switch status {
        case .satisfied: "satisfied"
        case .unsatisfied: "unsatisfied"
        case .requiresConnection: "requires connection"
        }
    }

    private func interfaceLabel(_ interface: PathInterfaceType) -> String {
        switch interface {
        case .wifi: "Wi-Fi"
        case .wiredEthernet: "Wired Ethernet"
        case .cellular: "Cellular"
        case .loopback: "Loopback"
        case .other: "Other"
        case .none: "none"
        }
    }

    private func flagsLabel(_ path: PathStatus) -> String? {
        let flags = [
            path.expensive ? "metered" : nil,
            path.constrained ? "Low Data Mode" : nil,
        ].compactMap(\.self)
        return flags.isEmpty ? nil : flags.joined(separator: ", ")
    }
}
