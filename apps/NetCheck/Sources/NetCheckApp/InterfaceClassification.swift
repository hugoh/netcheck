import NetStatus
import SwiftUI

/// UI presentation for `NetStatus.InterfaceClass` — the classification
/// logic itself lives in the engine.
extension InterfaceClass {
    var label: String {
        switch self {
        case .routable: "Up, routable"
        case .linkLocalOnly: "Up, link-local only"
        case .unaddressed: "Up, no address"
        case .down: "Down"
        }
    }

    var color: Color {
        switch self {
        case .routable: .green
        case .linkLocalOnly: .yellow
        case .unaddressed: .red
        case .down: .secondary
        }
    }
}

func classify(_ interface: NetInterface) -> InterfaceClass {
    interface.classification
}
