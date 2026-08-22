import SwiftUI

/// Mirrors `netstatus::interfaces::InterfaceClass` (Rust) — same online-to-
/// offline ordering, same up/address-based classification rules.
enum InterfaceClass: Int, Comparable {
    case routable = 0
    case linkLocalOnly = 1
    case unaddressed = 2
    case down = 3

    static func < (lhs: InterfaceClass, rhs: InterfaceClass) -> Bool {
        lhs.rawValue < rhs.rawValue
    }

    var label: String {
        switch self {
        case .routable: return "Up, routable"
        case .linkLocalOnly: return "Up, link-local only"
        case .unaddressed: return "Up, no address"
        case .down: return "Down"
        }
    }

    var color: Color {
        switch self {
        case .routable: return .green
        case .linkLocalOnly: return .yellow
        case .unaddressed: return .red
        case .down: return .secondary
        }
    }
}

func isLinkLocalAddress(_ address: String) -> Bool {
    let host = address.split(separator: "/").first.map(String.init) ?? address
    let lower = host.lowercased()
    return lower.hasPrefix("fe80:") || lower.hasPrefix("169.254.")
}

func classify(_ iface: NetInterface) -> InterfaceClass {
    if !iface.up {
        return .down
    }
    if iface.addresses.isEmpty {
        return .unaddressed
    }
    if iface.addresses.allSatisfy(isLinkLocalAddress) {
        return .linkLocalOnly
    }
    return .routable
}
