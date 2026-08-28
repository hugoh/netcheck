import Darwin
import Foundation

/// Coarse operational classification of an interface, from its up/down
/// state and the routability of its addresses. Ordered online-to-offline
/// so a sort surfaces the best interface first.
public enum InterfaceClass: Int, Comparable, Sendable {
    case routable = 0
    case linkLocalOnly = 1
    case unaddressed = 2
    case down = 3

    public static func < (lhs: InterfaceClass, rhs: InterfaceClass) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

/// True for an IPv4 self-assigned (169.254/16) or IPv6 link-local
/// (fe80::/10, matched by the `fe80:` textual prefix) address. Accepts an
/// optional `/prefix` suffix.
func isLinkLocal(_ address: String) -> Bool {
    let host = address.split(separator: "/", maxSplits: 1).first.map(String.init) ?? address
    let lower = host.lowercased()
    return lower.hasPrefix("fe80:") || lower.hasPrefix("169.254.")
}

public extension NetInterface {
    var classification: InterfaceClass {
        if !up {
            return .down
        }
        if addresses.isEmpty {
            return .unaddressed
        }
        if addresses.allSatisfy(isLinkLocal) {
            return .linkLocalOnly
        }
        return .routable
    }
}

extension Probe {
    /// Every network interface on the host, via `getifaddrs(3)`:
    /// `up` = `IFF_UP && IFF_RUNNING`, `loopback` = `IFF_LOOPBACK`, and
    /// each address rendered as `<addr>/<prefixlen>` with the IPv4
    /// addresses first, then IPv6, in `getifaddrs` order, de-duplicated.
    public static func listInterfaces() -> [NetInterface] {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0 else { return [] }
        defer { freeifaddrs(head) }

        struct Accumulator {
            var up = false
            var loopback = false
            var v4: [String] = []
            var v6: [String] = []
        }

        var order: [String] = []
        var byName: [String: Accumulator] = [:]

        var cursor = head
        while let entry = cursor {
            defer { cursor = entry.pointee.ifa_next }

            let name = String(cString: entry.pointee.ifa_name)
            let flags = Int32(bitPattern: entry.pointee.ifa_flags)
            if byName[name] == nil {
                order.append(name)
                byName[name] = Accumulator()
            }
            byName[name]?.up = (flags & IFF_UP) != 0 && (flags & IFF_RUNNING) != 0
            byName[name]?.loopback = (flags & IFF_LOOPBACK) != 0

            guard let sockaddr = entry.pointee.ifa_addr else { continue }
            let family = Int32(sockaddr.pointee.sa_family)
            guard family == AF_INET || family == AF_INET6, let ip = Self.ipString(sockaddr)
            else { continue }

            // A netmask sockaddr that is absent, or not of the address's
            // own family, is treated as unspecified (prefix 0) rather than
            // as a host route — this is how a `127.0.0.2` loopback alias
            // reads as `/0`.
            let prefix: Int = if let netmask = entry.pointee.ifa_netmask,
                                 Int32(netmask.pointee.sa_family) == family {
                Self.prefixLength(netmask, family: family)
            } else {
                0
            }
            let rendered = "\(ip)/\(prefix)"

            if family == AF_INET {
                if byName[name]?.v4.contains(rendered) == false {
                    byName[name]?.v4.append(rendered)
                }
            } else {
                if byName[name]?.v6.contains(rendered) == false {
                    byName[name]?.v6.append(rendered)
                }
            }
        }

        return order.compactMap { name in
            guard let acc = byName[name] else { return nil }
            return NetInterface(
                name: name, up: acc.up, loopback: acc.loopback, addresses: acc.v4 + acc.v6
            )
        }
    }

    /// Which IP families have a routable (non-link-local) address on an up,
    /// non-loopback interface.
    public static func ipStack(_ interfaces: [NetInterface]) -> IpStack {
        var hasV4 = false
        var hasV6 = false
        for iface in interfaces where iface.up && !iface.loopback {
            for addr in iface.addresses where !isLinkLocal(addr) {
                if addr.contains(":") {
                    hasV6 = true
                } else {
                    hasV4 = true
                }
            }
        }
        switch (hasV4, hasV6) {
        case (true, true): return .dualStack
        case (true, false): return .ipv4Only
        case (false, true): return .ipv6Only
        case (false, false): return .none
        }
    }

    static func ipString(_ sockaddr: UnsafePointer<sockaddr>) -> String? {
        var buffer = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
        switch Int32(sockaddr.pointee.sa_family) {
        case AF_INET:
            return sockaddr.withMemoryRebound(to: sockaddr_in.self, capacity: 1) { pointer in
                var addr = pointer.pointee.sin_addr
                return inet_ntop(AF_INET, &addr, &buffer, socklen_t(buffer.count))
                    .map { String(cString: $0) }
            }
        case AF_INET6:
            return sockaddr.withMemoryRebound(to: sockaddr_in6.self, capacity: 1) { pointer in
                var addr = pointer.pointee.sin6_addr
                return inet_ntop(AF_INET6, &addr, &buffer, socklen_t(buffer.count))
                    .map { String(cString: $0) }
            }
        default:
            return nil
        }
    }

    /// Leading-ones count of the netmask, honoring the BSD convention that
    /// a netmask `sockaddr` may be truncated to `sa_len` bytes — the bytes
    /// past `sa_len` are implicitly zero.
    static func prefixLength(_ mask: UnsafePointer<sockaddr>, family: Int32) -> Int {
        let addrOffset = family == AF_INET ? 4 : 8
        let addrWidth = family == AF_INET ? 4 : 16
        let present = max(0, min(Int(mask.pointee.sa_len), addrOffset + addrWidth) - addrOffset)
        guard present > 0 else { return 0 }
        return mask.withMemoryRebound(to: UInt8.self, capacity: addrOffset + present) { bytes in
            var leadingOnes = 0
            for i in 0 ..< addrWidth {
                let byte: UInt8 = i < present ? bytes[addrOffset + i] : 0
                leadingOnes += (~byte).leadingZeroBitCount
                if byte != 0xFF {
                    break
                }
            }
            return leadingOnes
        }
    }
}
